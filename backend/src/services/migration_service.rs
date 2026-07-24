//! F4.1 — Orchard → Ironwood migration engine (PRD-F4, docs/2026-07-24/).
//!
//! Lifecycle: create plan → (policy pivot) approve → execute → the
//! `MigrationExecutor` background worker drives scheduled batches to
//! completion. An approval covers the run's whole scheduling window;
//! cancel is the only way to stop remaining batches.
//!
//! The on-chain leg reuses the wallet service's real privacy-transfer path
//! (proposal → execute) with the wallet's own unified address as recipient.
//! Pre-NU6.3 that is an in-pool Orchard self-transfer; once F4.0's
//! height-aware builder lands, the identical call produces the
//! turnstile-crossing Orchard → Ironwood transaction. No mocks anywhere:
//! an RPC or proof failure marks the item `failed` with the real error.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use rust_decimal::Decimal;

use crate::db::models_f4::{
    CreateMigrationRunRequest, MigrationRun, MigrationRunSummary, MigrationStatusResponse,
};
use crate::db::repositories::{ApprovalPolicyRepository, MigrationRepository, WalletRepository};
use crate::error::{AppError, AppResult};
use crate::services::WalletService;

/// Private-mode defaults (PRD-F4 D4: 4–8 batches over ~48 h; tune from
/// on-chain migration traffic after launch).
const DEFAULT_PRIVATE_BATCHES: u32 = 6;
const DEFAULT_PRIVATE_WINDOW_HOURS: u32 = 48;
const MAX_BATCHES: u32 = 50;
const MAX_WINDOW_HOURS: u32 = 24 * 14;
/// Per-batch fee headroom reserved out of the spendable balance. ZIP-317
/// conventional fee for a small shielded tx is 10k–20k zatoshis; the real
/// fee is computed by the proposal at execution time — this only pads the
/// plan so the last batch is not starved.
const FEE_HEADROOM_ZATOSHIS: u64 = 20_000;
/// Refuse to plan a batch below this dust floor.
const MIN_BATCH_ZATOSHIS: u64 = 100_000;

const ZATOSHIS_PER_ZEC: u64 = 100_000_000;

pub struct MigrationService {
    repo: MigrationRepository,
    wallet_repo: WalletRepository,
    wallet_service: Arc<WalletService>,
    policy_repo: ApprovalPolicyRepository,
}

impl MigrationService {
    pub fn new(
        repo: MigrationRepository,
        wallet_repo: WalletRepository,
        wallet_service: Arc<WalletService>,
        policy_repo: ApprovalPolicyRepository,
    ) -> Self {
        Self {
            repo,
            wallet_repo,
            wallet_service,
            policy_repo,
        }
    }

    // -----------------------------------------------------------------------
    // create_run — snapshot the spendable shielded balance, split it into a
    // batch plan, persist run + items atomically
    // -----------------------------------------------------------------------

    pub async fn create_run(
        &self,
        req: CreateMigrationRunRequest,
        user_id: i32,
    ) -> AppResult<MigrationRunSummary> {
        let wallet = self
            .wallet_repo
            .find_by_id(req.source_wallet_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!("wallet {} not found", req.source_wallet_id))
            })?;
        if wallet.chain != "zcash" {
            return Err(AppError::ValidationError(
                "migration runs are only defined for zcash wallets".to_string(),
            ));
        }

        if let Some(active) = self.repo.find_active_run_for_wallet(wallet.id).await? {
            return Err(AppError::ValidationError(format!(
                "wallet {} already has migration run {} in state '{}'",
                wallet.id, active.id, active.status
            )));
        }

        let (batches, window_hours) = match req.mode.as_str() {
            "immediate" => (1, 0),
            "private" => (
                req.batch_count.unwrap_or(DEFAULT_PRIVATE_BATCHES),
                req.window_hours.unwrap_or(DEFAULT_PRIVATE_WINDOW_HOURS),
            ),
            other => {
                return Err(AppError::ValidationError(format!(
                    "mode must be 'immediate' or 'private', got '{}'",
                    other
                )));
            }
        };
        if batches == 0 || batches > MAX_BATCHES {
            return Err(AppError::ValidationError(format!(
                "batch_count must be between 1 and {}",
                MAX_BATCHES
            )));
        }
        if req.mode == "private" && (window_hours == 0 || window_hours > MAX_WINDOW_HOURS) {
            return Err(AppError::ValidationError(format!(
                "window_hours must be between 1 and {}",
                MAX_WINDOW_HOURS
            )));
        }

        // Live balance, not a cached figure — the plan totals what is
        // actually spendable right now. The executor re-checks per batch.
        let balance = self.wallet_service.get_shielded_balance(wallet.id).await?;
        let headroom = FEE_HEADROOM_ZATOSHIS.saturating_mul(batches as u64);
        let plannable = balance.spendable_zatoshis.saturating_sub(headroom);
        if plannable < MIN_BATCH_ZATOSHIS as u64 * batches as u64 {
            return Err(AppError::InsufficientBalance(format!(
                "spendable shielded balance {} zatoshis cannot fund {} batches \
                 (needs ≥ {} per batch after {} zatoshis fee headroom)",
                balance.spendable_zatoshis,
                batches,
                MIN_BATCH_ZATOSHIS,
                headroom
            )));
        }

        let mut rng = rand::thread_rng();
        let amounts = split_amount_randomized(plannable, batches, &mut rng);
        let offsets = schedule_offsets_secs(batches, window_hours, &mut rng);

        let now = Utc::now();
        let items: Vec<(Decimal, Option<DateTime<Utc>>)> = amounts
            .iter()
            .zip(offsets.iter())
            .map(|(&zat, &offset)| {
                let scheduled = if offset == 0 {
                    None
                } else {
                    Some(now + Duration::seconds(offset as i64))
                };
                (zatoshis_to_zec(zat), scheduled)
            })
            .collect();

        let total: Decimal = items.iter().map(|(a, _)| *a).sum();
        let run_id = self
            .repo
            .create_run_with_items(
                wallet.id,
                &req.mode,
                batches,
                window_hours,
                total,
                user_id,
                req.notes.as_deref(),
                &items,
            )
            .await?;

        tracing::info!(
            "[migration] run {} created: wallet={} mode={} batches={} window={}h total={} ZEC",
            run_id,
            wallet.id,
            req.mode,
            batches,
            window_hours,
            total
        );
        self.get_run_summary(run_id).await
    }

    // -----------------------------------------------------------------------
    // Reads
    // -----------------------------------------------------------------------

    pub async fn get_run_summary(&self, run_id: i32) -> AppResult<MigrationRunSummary> {
        let run = self
            .repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("migration_run {} not found", run_id)))?;
        let items = self.repo.list_items_by_run(run_id).await?;
        Ok(MigrationRunSummary { run, items })
    }

    pub async fn list_runs(&self, limit: i32, offset: i32) -> AppResult<Vec<MigrationRun>> {
        self.repo.list_runs(limit.clamp(1, 200), offset.max(0)).await
    }

    /// Wallet-page banner: legacy-pool holdings + any active run.
    pub async fn migration_status(&self, wallet_id: i32) -> AppResult<MigrationStatusResponse> {
        let balance = self.wallet_service.get_shielded_balance(wallet_id).await?;
        let active = self.repo.find_active_run_for_wallet(wallet_id).await?;
        Ok(MigrationStatusResponse {
            wallet_id,
            spendable_zatoshis: balance.spendable_zatoshis,
            total_zatoshis: balance.total_zatoshis,
            unspent_note_count: balance.note_count,
            active_run_id: active.as_ref().map(|r| r.id),
            active_run_status: active.map(|r| r.status),
        })
    }

    // -----------------------------------------------------------------------
    // execute_run — F2.1 policy hook, then hand the run to the executor.
    // Approval semantics (PRD-F4 F4.1.5): one approval covers the whole
    // scheduling window; batches run unattended; cancel is the only stop.
    // -----------------------------------------------------------------------

    pub async fn execute_run(&self, run_id: i32, user_id: i32) -> AppResult<ExecuteMigrationOutcome> {
        let run = self
            .repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("migration_run {} not found", run_id)))?;

        match run.status.as_str() {
            "pending" | "approved" => {}
            "awaiting_approval" => {
                return Err(AppError::ValidationError(
                    "migration_run is awaiting approval and cannot be executed".to_string(),
                ));
            }
            other => {
                return Err(AppError::ValidationError(format!(
                    "migration_run is in state '{}' and cannot be executed",
                    other
                )));
            }
        }

        // Policy check mirrors payroll: evaluated on execute so a policy
        // added after plan creation still applies; an approved run skips it
        // (the approval is the explicit override).
        if run.status == "pending" {
            let wallet = self
                .wallet_repo
                .find_by_id(run.source_wallet_id)
                .await?
                .ok_or_else(|| {
                    AppError::InternalError(format!(
                        "source_wallet {} vanished after run was created",
                        run.source_wallet_id
                    ))
                })?;
            let matching = self
                .policy_repo
                .find_matching(&wallet.chain, "ZEC", wallet.id, user_id)
                .await?
                .into_iter()
                .find(|p| run.total_amount >= p.amount_threshold);

            if let Some(policy) = matching {
                tracing::info!(
                    "[migration] run {} total {} matched policy {} (threshold {}) -> awaiting_approval",
                    run.id,
                    run.total_amount,
                    policy.id,
                    policy.amount_threshold
                );
                self.repo
                    .update_run_status(run.id, "awaiting_approval", None, None)
                    .await?;
                return Ok(ExecuteMigrationOutcome::AwaitingApproval {
                    run_id: run.id,
                    policy_id: policy.id,
                    threshold: policy.amount_threshold,
                });
            }
        }

        self.repo
            .update_run_status(run.id, "executing", Some(user_id), Some(Utc::now()))
            .await?;
        tracing::info!("[migration] run {} -> executing (scheduler takes over)", run.id);
        Ok(ExecuteMigrationOutcome::Executing { run_id: run.id })
    }

    pub async fn approve_run(&self, run_id: i32, user_id: i32) -> AppResult<MigrationRun> {
        let run = self
            .repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("migration_run {} not found", run_id)))?;
        if run.status != "awaiting_approval" {
            return Err(AppError::ValidationError(format!(
                "migration_run is in state '{}'; only 'awaiting_approval' runs may be approved",
                run.status
            )));
        }
        // The repo UPDATE enforces maker ≠ checker in SQL; report the reason
        // precisely when it refuses.
        if !self.repo.approve_run(run_id, user_id).await? {
            if run.created_by_user_id == user_id {
                return Err(AppError::Forbidden(
                    "maker cannot approve their own migration run".to_string(),
                ));
            }
            return Err(AppError::ValidationError(
                "migration_run left 'awaiting_approval' before the approval was recorded"
                    .to_string(),
            ));
        }
        self.repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::InternalError("run vanished after approve".to_string()))
    }

    pub async fn reject_run(
        &self,
        run_id: i32,
        user_id: i32,
        reason: &str,
    ) -> AppResult<MigrationRun> {
        let reason = reason.trim();
        if reason.chars().count() < 5 {
            return Err(AppError::ValidationError(
                "reject reason must be at least 5 characters".to_string(),
            ));
        }
        let run = self
            .repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("migration_run {} not found", run_id)))?;
        if run.status != "awaiting_approval" {
            return Err(AppError::ValidationError(format!(
                "migration_run is in state '{}'; only 'awaiting_approval' runs may be rejected",
                run.status
            )));
        }
        if !self.repo.reject_run(run_id, user_id, reason).await? {
            if run.created_by_user_id == user_id {
                return Err(AppError::Forbidden(
                    "maker cannot reject their own migration run".to_string(),
                ));
            }
            return Err(AppError::ValidationError(
                "migration_run left 'awaiting_approval' before the rejection was recorded"
                    .to_string(),
            ));
        }
        self.repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::InternalError("run vanished after reject".to_string()))
    }

    /// Cancel: the only way to stop remaining batches. Legal from every
    /// non-terminal state; already-submitted items are on-chain and stay.
    pub async fn cancel_run(&self, run_id: i32) -> AppResult<MigrationRun> {
        let run = self
            .repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("migration_run {} not found", run_id)))?;
        if !matches!(
            run.status.as_str(),
            "pending" | "awaiting_approval" | "approved" | "executing"
        ) {
            return Err(AppError::ValidationError(format!(
                "migration_run in terminal state '{}' cannot be canceled",
                run.status
            )));
        }
        let canceled_items = self.repo.cancel_pending_items(run_id).await?;
        self.repo
            .update_run_status(run_id, "canceled", None, None)
            .await?;
        tracing::info!(
            "[migration] run {} canceled ({} pending items stopped)",
            run_id,
            canceled_items
        );
        self.repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::InternalError("run vanished after cancel".to_string()))
    }

    pub async fn retry_item(&self, run_id: i32, item_id: i32) -> AppResult<()> {
        let run = self
            .repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("migration_run {} not found", run_id)))?;
        if run.status != "executing" && run.status != "partial" && run.status != "failed" {
            return Err(AppError::ValidationError(format!(
                "items can only be retried while the run is 'executing', 'partial' or 'failed' (got '{}')",
                run.status
            )));
        }
        let item = self
            .repo
            .find_item(run_id, item_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!("migration_item {} not found in run {}", item_id, run_id))
            })?;
        if item.status != "failed" {
            return Err(AppError::ValidationError(format!(
                "migration_item is in state '{}'; only 'failed' items may be retried",
                item.status
            )));
        }
        if !self.repo.reset_item_for_retry(item_id).await? {
            return Err(AppError::ValidationError(
                "migration_item left 'failed' before the retry was recorded".to_string(),
            ));
        }
        // A retried item on a folded run re-opens it for the scheduler.
        if run.status != "executing" {
            self.repo
                .update_run_status(run_id, "executing", None, None)
                .await?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ExecuteMigrationOutcome {
    AwaitingApproval {
        run_id: i32,
        policy_id: i32,
        threshold: Decimal,
    },
    Executing {
        run_id: i32,
    },
}

pub fn zatoshis_to_zec(zatoshis: u64) -> Decimal {
    Decimal::from(zatoshis) / Decimal::from(ZATOSHIS_PER_ZEC)
}

pub fn zec_to_zatoshis(zec: Decimal) -> AppResult<u64> {
    use rust_decimal::prelude::ToPrimitive;
    (zec * Decimal::from(ZATOSHIS_PER_ZEC))
        .trunc()
        .to_u64()
        .ok_or_else(|| AppError::ValidationError(format!("amount {} ZEC out of range", zec)))
}

// ---------------------------------------------------------------------------
// Plan generation — pure functions, deterministic given an RNG, unit-tested
// ---------------------------------------------------------------------------

/// Split `total` zatoshis into `batches` amounts with ±40% jitter around the
/// even share, so migration batches don't carry an equal-amount fingerprint
/// on the turnstile (PRD-F4 §6: MVP is simple randomized splitting; fixed
/// denominations are P2). Every batch stays ≥ MIN_BATCH_ZATOSHIS and the
/// last batch absorbs rounding so the sum is exactly `total`.
pub fn split_amount_randomized<R: Rng>(total: u64, batches: u32, rng: &mut R) -> Vec<u64> {
    let n = batches.max(1) as usize;
    if n == 1 {
        return vec![total];
    }

    let weights: Vec<f64> = (0..n).map(|_| 1.0 + rng.gen_range(-0.4..0.4)).collect();
    let weight_sum: f64 = weights.iter().sum();

    let mut amounts: Vec<u64> = weights
        .iter()
        .map(|w| ((total as f64) * w / weight_sum) as u64)
        .map(|a| a.max(MIN_BATCH_ZATOSHIS))
        .collect();

    // Force the exact total onto the last batch; if clamping pushed the sum
    // over the total, walk the overshoot back off the largest batches.
    let sum_except_last: u64 = amounts[..n - 1].iter().sum();
    if sum_except_last + MIN_BATCH_ZATOSHIS <= total {
        amounts[n - 1] = total - sum_except_last;
    } else {
        let mut overshoot = sum_except_last + MIN_BATCH_ZATOSHIS - total;
        amounts[n - 1] = MIN_BATCH_ZATOSHIS;
        for a in amounts[..n - 1].iter_mut().rev() {
            let take = overshoot.min(a.saturating_sub(MIN_BATCH_ZATOSHIS));
            *a -= take;
            overshoot -= take;
            if overshoot == 0 {
                break;
            }
        }
    }
    amounts
}

/// Schedule offsets (seconds from now) for `batches` over `window_hours`.
/// First batch fires immediately; the rest sit near even spacing with ±30%
/// slot jitter so the sequence doesn't tick like a bot (PRD-F4 §6 time
/// correlation). Output is strictly non-decreasing.
pub fn schedule_offsets_secs<R: Rng>(batches: u32, window_hours: u32, rng: &mut R) -> Vec<u64> {
    let n = batches.max(1) as usize;
    if n == 1 || window_hours == 0 {
        return vec![0; n];
    }
    let window_secs = window_hours as f64 * 3600.0;
    let slot = window_secs / (n as f64 - 1.0);

    let mut offsets: Vec<u64> = (0..n)
        .map(|i| {
            if i == 0 {
                0
            } else {
                let base = slot * i as f64;
                let jitter = rng.gen_range(-0.3..0.3) * slot;
                (base + jitter).clamp(0.0, window_secs) as u64
            }
        })
        .collect();
    offsets.sort_unstable();
    offsets
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[test]
    fn split_preserves_total_and_dust_floor() {
        let mut rng = StdRng::seed_from_u64(42);
        for batches in [1u32, 2, 6, 13, 50] {
            for total in [
                MIN_BATCH_ZATOSHIS * 50 * 2,
                123_456_789,
                5_000_000_000,
                u32::MAX as u64,
            ] {
                if total < MIN_BATCH_ZATOSHIS * batches as u64 {
                    continue;
                }
                let amounts = split_amount_randomized(total, batches, &mut rng);
                assert_eq!(amounts.len(), batches as usize);
                assert_eq!(amounts.iter().sum::<u64>(), total, "batches={batches} total={total}");
                assert!(amounts.iter().all(|&a| a >= MIN_BATCH_ZATOSHIS));
            }
        }
    }

    #[test]
    fn split_single_batch_is_identity() {
        let mut rng = StdRng::seed_from_u64(7);
        assert_eq!(split_amount_randomized(12_345, 1, &mut rng), vec![12_345]);
    }

    #[test]
    fn split_amounts_are_not_uniform() {
        // The anti-fingerprint property: batches must not all be equal.
        let mut rng = StdRng::seed_from_u64(1);
        let amounts = split_amount_randomized(6_000_000_000, 6, &mut rng);
        let first = amounts[0];
        assert!(
            amounts.iter().any(|&a| a != first),
            "randomized split produced equal batches: {:?}",
            amounts
        );
    }

    #[test]
    fn schedule_first_is_immediate_rest_in_window() {
        let mut rng = StdRng::seed_from_u64(42);
        let offsets = schedule_offsets_secs(6, 48, &mut rng);
        assert_eq!(offsets.len(), 6);
        assert_eq!(offsets[0], 0, "first batch fires immediately");
        assert!(offsets.windows(2).all(|w| w[0] <= w[1]), "non-decreasing");
        assert!(*offsets.last().unwrap() <= 48 * 3600);
        // Spread sanity: last batch lands in the back half of the window.
        assert!(*offsets.last().unwrap() >= 24 * 3600, "got {:?}", offsets);
    }

    #[test]
    fn schedule_immediate_mode_is_all_zero() {
        let mut rng = StdRng::seed_from_u64(42);
        assert_eq!(schedule_offsets_secs(1, 0, &mut rng), vec![0]);
        assert_eq!(schedule_offsets_secs(4, 0, &mut rng), vec![0; 4]);
    }

    #[test]
    fn zatoshi_zec_roundtrip() {
        for zat in [0u64, 1, 100_000_000, 123_456_789, 2_100_000_000_000_000] {
            let zec = zatoshis_to_zec(zat);
            assert_eq!(zec_to_zatoshis(zec).unwrap(), zat, "zat={zat} zec={zec}");
        }
    }
}
