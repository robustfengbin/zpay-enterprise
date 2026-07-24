//! F4.2 — generic batch privacy transfer (PRD-F4 §5).
//!
//! Generalizes F3.1 payroll to arbitrary recipient lists (vendors,
//! rebates, airdrop-style payouts) and adds the privacy-scheduling
//! dimension from the migration engine: staggered batches over a time
//! window, plus per-transfer amount caps with randomized sub-splitting.
//!
//! Lifecycle mirrors F4.1 exactly (create → policy pivot → approve →
//! execute → RunExecutor drives batches): one approval covers the whole
//! window, cancel is the only stop, failed items retry individually.
//! Recipients must be Orchard-capable unified addresses — this is a
//! *shielded* batch facility; transparent payouts belong to the plain
//! transfer flow.

use std::collections::HashSet;
use std::str::FromStr;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use rand::seq::SliceRandom;
use rand::Rng;
use rust_decimal::Decimal;

use crate::blockchain::zcash::orchard::address::OrchardAddressManager;
use crate::db::models_f4::{
    BatchTransferRun, BatchTransferRunSummary, CreateBatchTransferRunRequest,
    CreateBatchTransferRunResponse,
};
use crate::db::models_m1::ValidationError;
use crate::db::repositories::batch_transfer_repo::PlannedBatchItem;
use crate::db::repositories::{ApprovalPolicyRepository, BatchTransferRepository, WalletRepository};
use crate::error::{AppError, AppResult};
use crate::services::migration_service::{schedule_offsets_secs, zatoshis_to_zec, zec_to_zatoshis};
use crate::services::WalletService;

/// Staggered-mode defaults: lighter than a whole-treasury migration —
/// payouts usually want same-day settlement.
const DEFAULT_STAGGERED_BATCHES: u32 = 4;
const DEFAULT_STAGGERED_WINDOW_HOURS: u32 = 24;
const MAX_BATCHES: u32 = 50;
const MAX_WINDOW_HOURS: u32 = 24 * 14;
/// Input rows per run; splitting may grow the planned item list up to
/// MAX_PLANNED_ITEMS.
const MAX_INPUT_ROWS: usize = 500;
const MAX_PLANNED_ITEMS: usize = 1000;
/// Sub-transfers produced by a cap split stay at or above this floor, so
/// the cap itself must be at least twice the floor for ceil-division to
/// stay feasible.
const MIN_SPLIT_ZATOSHIS: u64 = 100_000;
const MIN_CAP_ZATOSHIS: u64 = 2 * MIN_SPLIT_ZATOSHIS;
/// Same per-batch fee padding as the migration planner (ZIP-317 scale).
const FEE_HEADROOM_ZATOSHIS: u64 = 20_000;
/// On-chain memo field is 512 bytes.
const MAX_MEMO_BYTES: usize = 512;

pub struct BatchTransferService {
    repo: BatchTransferRepository,
    wallet_repo: WalletRepository,
    wallet_service: Arc<WalletService>,
    policy_repo: ApprovalPolicyRepository,
}

impl BatchTransferService {
    pub fn new(
        repo: BatchTransferRepository,
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
    // create_run — two-level validation (row errors collected, then run-level
    // balance check), cap splitting, schedule assignment, atomic insert
    // -----------------------------------------------------------------------

    pub async fn create_run(
        &self,
        req: CreateBatchTransferRunRequest,
        user_id: i32,
    ) -> AppResult<CreateBatchTransferRunResponse> {
        let title = req.title.trim();
        if title.is_empty() || title.chars().count() > 120 {
            return Err(AppError::ValidationError(
                "title must be 1–120 characters".to_string(),
            ));
        }
        if req.items.is_empty() {
            return Err(AppError::ValidationError(
                "items cannot be empty".to_string(),
            ));
        }
        if req.items.len() > MAX_INPUT_ROWS {
            return Err(AppError::ValidationError(format!(
                "at most {} recipient rows per run (got {})",
                MAX_INPUT_ROWS,
                req.items.len()
            )));
        }

        let (batches, window_hours) = match req.privacy_mode.as_str() {
            "off" => (1, 0),
            "staggered" => (
                req.batch_count.unwrap_or(DEFAULT_STAGGERED_BATCHES),
                req.window_hours.unwrap_or(DEFAULT_STAGGERED_WINDOW_HOURS),
            ),
            other => {
                return Err(AppError::ValidationError(format!(
                    "privacy_mode must be 'off' or 'staggered', got '{}'",
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
        if req.privacy_mode == "staggered" && (window_hours == 0 || window_hours > MAX_WINDOW_HOURS)
        {
            return Err(AppError::ValidationError(format!(
                "window_hours must be between 1 and {}",
                MAX_WINDOW_HOURS
            )));
        }

        // Per-transfer cap only makes sense with privacy scheduling on.
        let cap_zatoshis: Option<u64> = match (req.privacy_mode.as_str(), &req.max_per_transfer) {
            (_, None) => None,
            ("off", Some(_)) => {
                return Err(AppError::ValidationError(
                    "max_per_transfer requires privacy_mode='staggered'".to_string(),
                ));
            }
            ("staggered", Some(raw)) => {
                let cap = Decimal::from_str(raw).map_err(|e| {
                    AppError::ValidationError(format!("invalid max_per_transfer: {}", e))
                })?;
                let cap_zat = zec_to_zatoshis(cap)?;
                if cap_zat < MIN_CAP_ZATOSHIS {
                    return Err(AppError::ValidationError(format!(
                        "max_per_transfer must be at least {} ZEC",
                        zatoshis_to_zec(MIN_CAP_ZATOSHIS)
                    )));
                }
                Some(cap_zat)
            }
            _ => unreachable!("privacy_mode validated above"),
        };

        let wallet = self
            .wallet_repo
            .find_by_id(req.source_wallet_id)
            .await?
            .ok_or_else(|| {
                AppError::NotFound(format!("wallet {} not found", req.source_wallet_id))
            })?;
        if wallet.chain != "zcash" {
            return Err(AppError::ValidationError(
                "batch privacy transfers are only defined for zcash wallets".to_string(),
            ));
        }

        // Row-level validation — collect every error so the operator fixes
        // the whole CSV in one pass (same contract as payroll).
        let mut validation_errors: Vec<ValidationError> = Vec::new();
        let mut seen_rows: HashSet<(String, String, Option<String>)> = HashSet::new();
        let mut resolved: Vec<(String, u64, Option<String>)> = Vec::with_capacity(req.items.len());

        for (idx, input) in req.items.iter().enumerate() {
            let recipient = input.recipient_address.trim();

            // New-pool rule: the recipient must carry an Orchard receiver.
            // The same decode is what the transfer path itself uses, so a
            // row that passes here cannot fail address parsing at execution.
            if let Err(e) = OrchardAddressManager::extract_orchard_address(recipient) {
                validation_errors.push(ValidationError {
                    row_index: idx,
                    field: "recipient_address".to_string(),
                    message: format!(
                        "recipient must be an Orchard-capable unified address: {}",
                        e
                    ),
                });
                continue;
            }

            let amount_zat = match Decimal::from_str(&input.amount)
                .map_err(|e| format!("invalid amount: {}", e))
                .and_then(|d| {
                    if d <= Decimal::ZERO {
                        Err("amount must be positive".to_string())
                    } else {
                        zec_to_zatoshis(d).map_err(|e| e.to_string())
                    }
                }) {
                Ok(z) if z == 0 => {
                    validation_errors.push(ValidationError {
                        row_index: idx,
                        field: "amount".to_string(),
                        message: "amount is below 1 zatoshi".to_string(),
                    });
                    continue;
                }
                Ok(z) => z,
                Err(msg) => {
                    validation_errors.push(ValidationError {
                        row_index: idx,
                        field: "amount".to_string(),
                        message: msg,
                    });
                    continue;
                }
            };

            if let Some(memo) = &input.memo {
                if memo.as_bytes().len() > MAX_MEMO_BYTES {
                    validation_errors.push(ValidationError {
                        row_index: idx,
                        field: "memo".to_string(),
                        message: format!("memo exceeds {} bytes", MAX_MEMO_BYTES),
                    });
                    continue;
                }
            }

            // Dedup: an identical (recipient, amount, memo) row is almost
            // always a copy-paste error in the CSV; a legitimate double
            // payment can differ by memo.
            let key = (
                recipient.to_string(),
                input.amount.trim().to_string(),
                input.memo.clone(),
            );
            if !seen_rows.insert(key) {
                validation_errors.push(ValidationError {
                    row_index: idx,
                    field: "recipient_address".to_string(),
                    message: "duplicate row (same recipient, amount and memo)".to_string(),
                });
                continue;
            }

            resolved.push((recipient.to_string(), amount_zat, input.memo.clone()));
        }

        if !validation_errors.is_empty() {
            return Ok(CreateBatchTransferRunResponse {
                run_id: 0,
                item_count: 0,
                validation_errors,
            });
        }

        // Cap splitting (PRD F4.2.3): rows above the cap become several
        // randomized sub-transfers so payout sizes don't leak the original
        // row structure into note commitments.
        let mut rng = rand::thread_rng();
        let mut planned: Vec<(String, u64, Option<String>)> = Vec::with_capacity(resolved.len());
        for (recipient, zat, memo) in resolved {
            match cap_zatoshis {
                Some(cap) if zat > cap => {
                    for part in split_with_cap(zat, cap, &mut rng) {
                        planned.push((recipient.clone(), part, memo.clone()));
                    }
                }
                _ => planned.push((recipient, zat, memo)),
            }
        }
        if planned.len() > MAX_PLANNED_ITEMS {
            return Err(AppError::ValidationError(format!(
                "cap splitting produced {} transfers (max {}); raise max_per_transfer or split the run",
                planned.len(),
                MAX_PLANNED_ITEMS
            )));
        }

        // Run-level check: total + fee headroom within the live spendable
        // shielded balance. The executor re-checks reality per item.
        let total_zat: u64 = planned.iter().map(|(_, z, _)| *z).sum();
        let headroom = FEE_HEADROOM_ZATOSHIS.saturating_mul(planned.len() as u64);
        let balance = self.wallet_service.get_shielded_balance(wallet.id).await?;
        if total_zat.saturating_add(headroom) > balance.spendable_zatoshis {
            return Err(AppError::InsufficientBalance(format!(
                "total {} zatoshis + {} fee headroom exceeds spendable shielded balance {}",
                total_zat, headroom, balance.spendable_zatoshis
            )));
        }

        // Schedule assignment: shuffle so CSV order doesn't map onto time
        // slots, then chunk into `batches` groups over the window. Group 0
        // fires immediately (NULL schedule), the rest carry jittered slots.
        planned.shuffle(&mut rng);
        let offsets = schedule_offsets_secs(batches, window_hours, &mut rng);
        let group_size = planned.len().div_ceil(batches as usize);
        let now = Utc::now();
        let items: Vec<PlannedBatchItem> = planned
            .into_iter()
            .enumerate()
            .map(|(i, (recipient, zat, memo))| {
                let offset = offsets[(i / group_size.max(1)).min(offsets.len() - 1)];
                let scheduled: Option<DateTime<Utc>> = if offset == 0 {
                    None
                } else {
                    Some(now + Duration::seconds(offset as i64))
                };
                (recipient, zatoshis_to_zec(zat), memo, scheduled)
            })
            .collect();

        let total: Decimal = items.iter().map(|(_, a, _, _)| *a).sum();
        let run_id = self
            .repo
            .create_run_with_items(
                title,
                wallet.id,
                &req.privacy_mode,
                batches,
                window_hours,
                total,
                user_id,
                req.notes.as_deref(),
                &items,
            )
            .await?;

        tracing::info!(
            "[batch-transfer] run {} created: wallet={} mode={} batches={} window={}h items={} total={} ZEC",
            run_id,
            wallet.id,
            req.privacy_mode,
            batches,
            window_hours,
            items.len(),
            total
        );
        Ok(CreateBatchTransferRunResponse {
            run_id,
            item_count: items.len() as i32,
            validation_errors: Vec::new(),
        })
    }

    // -----------------------------------------------------------------------
    // Reads
    // -----------------------------------------------------------------------

    pub async fn get_run_summary(&self, run_id: i32) -> AppResult<BatchTransferRunSummary> {
        let run = self
            .repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("batch_transfer_run {} not found", run_id)))?;
        let items = self.repo.list_items_by_run(run_id).await?;
        Ok(BatchTransferRunSummary { run, items })
    }

    pub async fn list_runs(&self, limit: i32, offset: i32) -> AppResult<Vec<BatchTransferRun>> {
        self.repo.list_runs(limit.clamp(1, 200), offset.max(0)).await
    }

    // -----------------------------------------------------------------------
    // execute / approve / reject / cancel / retry — F4.1 semantics verbatim
    // -----------------------------------------------------------------------

    pub async fn execute_run(
        &self,
        run_id: i32,
        user_id: i32,
    ) -> AppResult<ExecuteBatchTransferOutcome> {
        let run = self
            .repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("batch_transfer_run {} not found", run_id)))?;

        match run.status.as_str() {
            "pending" | "approved" => {}
            "awaiting_approval" => {
                return Err(AppError::ValidationError(
                    "batch_transfer_run is awaiting approval and cannot be executed".to_string(),
                ));
            }
            other => {
                return Err(AppError::ValidationError(format!(
                    "batch_transfer_run is in state '{}' and cannot be executed",
                    other
                )));
            }
        }

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
                    "[batch-transfer] run {} total {} matched policy {} (threshold {}) -> awaiting_approval",
                    run.id,
                    run.total_amount,
                    policy.id,
                    policy.amount_threshold
                );
                self.repo
                    .update_run_status(run.id, "awaiting_approval", None, None)
                    .await?;
                return Ok(ExecuteBatchTransferOutcome::AwaitingApproval {
                    run_id: run.id,
                    policy_id: policy.id,
                    threshold: policy.amount_threshold,
                });
            }
        }

        self.repo
            .update_run_status(run.id, "executing", Some(user_id), Some(Utc::now()))
            .await?;
        tracing::info!(
            "[batch-transfer] run {} -> executing (scheduler takes over)",
            run.id
        );
        Ok(ExecuteBatchTransferOutcome::Executing { run_id: run.id })
    }

    pub async fn approve_run(&self, run_id: i32, user_id: i32) -> AppResult<BatchTransferRun> {
        let run = self
            .repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("batch_transfer_run {} not found", run_id)))?;
        if run.status != "awaiting_approval" {
            return Err(AppError::ValidationError(format!(
                "batch_transfer_run is in state '{}'; only 'awaiting_approval' runs may be approved",
                run.status
            )));
        }
        if !self.repo.approve_run(run_id, user_id).await? {
            if run.created_by_user_id == user_id {
                return Err(AppError::Forbidden(
                    "maker cannot approve their own batch transfer run".to_string(),
                ));
            }
            return Err(AppError::ValidationError(
                "batch_transfer_run left 'awaiting_approval' before the approval was recorded"
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
    ) -> AppResult<BatchTransferRun> {
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
            .ok_or_else(|| AppError::NotFound(format!("batch_transfer_run {} not found", run_id)))?;
        if run.status != "awaiting_approval" {
            return Err(AppError::ValidationError(format!(
                "batch_transfer_run is in state '{}'; only 'awaiting_approval' runs may be rejected",
                run.status
            )));
        }
        if !self.repo.reject_run(run_id, user_id, reason).await? {
            if run.created_by_user_id == user_id {
                return Err(AppError::Forbidden(
                    "maker cannot reject their own batch transfer run".to_string(),
                ));
            }
            return Err(AppError::ValidationError(
                "batch_transfer_run left 'awaiting_approval' before the rejection was recorded"
                    .to_string(),
            ));
        }
        self.repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::InternalError("run vanished after reject".to_string()))
    }

    pub async fn cancel_run(&self, run_id: i32) -> AppResult<BatchTransferRun> {
        let run = self
            .repo
            .find_run_by_id(run_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("batch_transfer_run {} not found", run_id)))?;
        if !matches!(
            run.status.as_str(),
            "pending" | "awaiting_approval" | "approved" | "executing"
        ) {
            return Err(AppError::ValidationError(format!(
                "batch_transfer_run in terminal state '{}' cannot be canceled",
                run.status
            )));
        }
        let canceled_items = self.repo.cancel_pending_items(run_id).await?;
        self.repo
            .update_run_status(run_id, "canceled", None, None)
            .await?;
        tracing::info!(
            "[batch-transfer] run {} canceled ({} pending items stopped)",
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
            .ok_or_else(|| AppError::NotFound(format!("batch_transfer_run {} not found", run_id)))?;
        if run.status != "executing" && run.status != "partial" && run.status != "failed" {
            return Err(AppError::ValidationError(format!(
                "items can only be retried while the run is 'executing', 'partial' or 'failed' (got '{}')",
                run.status
            )));
        }
        let item = self.repo.find_item(run_id, item_id).await?.ok_or_else(|| {
            AppError::NotFound(format!(
                "batch_transfer_item {} not found in run {}",
                item_id, run_id
            ))
        })?;
        if item.status != "failed" {
            return Err(AppError::ValidationError(format!(
                "batch_transfer_item is in state '{}'; only 'failed' items may be retried",
                item.status
            )));
        }
        if !self.repo.reset_item_for_retry(item_id).await? {
            return Err(AppError::ValidationError(
                "batch_transfer_item left 'failed' before the retry was recorded".to_string(),
            ));
        }
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
pub enum ExecuteBatchTransferOutcome {
    AwaitingApproval {
        run_id: i32,
        policy_id: i32,
        threshold: Decimal,
    },
    Executing {
        run_id: i32,
    },
}

// ---------------------------------------------------------------------------
// Cap splitting — pure function, unit-tested
// ---------------------------------------------------------------------------

/// Split `total` into randomized parts, each within
/// [MIN_SPLIT_ZATOSHIS, cap]. Caller guarantees `total > cap` and
/// `cap >= MIN_CAP_ZATOSHIS`, which makes ceil-division always feasible:
/// with n = ceil(total / cap), total > (n-1)·cap ≥ n·MIN_SPLIT for n ≥ 2.
/// Randomized weights avoid the equal-chunk fingerprint (same rationale as
/// the migration splitter); the final pass walks rounding drift onto parts
/// with headroom so the sum is exactly `total`.
pub fn split_with_cap<R: Rng>(total: u64, cap: u64, rng: &mut R) -> Vec<u64> {
    let mut n = total.div_ceil(cap).max(2);
    // When total nearly saturates n·cap (e.g. an exact multiple of the
    // cap), every part gets clamped to the cap and the split degenerates
    // to equal amounts — the exact fingerprint this exists to avoid. One
    // extra part restores jitter headroom.
    if total > n.saturating_mul(cap) / 10 * 9 {
        n += 1;
    }
    let n = n as usize;

    let weights: Vec<f64> = (0..n).map(|_| 1.0 + rng.gen_range(-0.4..0.4)).collect();
    let weight_sum: f64 = weights.iter().sum();

    let mut parts: Vec<u64> = weights
        .iter()
        .map(|w| ((total as f64) * w / weight_sum) as u64)
        .map(|a| a.clamp(MIN_SPLIT_ZATOSHIS, cap))
        .collect();

    // Fix the rounding/clamping drift: push the difference onto parts that
    // still have headroom toward their bound. Feasibility is guaranteed by
    // n·MIN ≤ total ≤ n·cap.
    let mut sum: u64 = parts.iter().sum();
    let mut i = 0;
    while sum != total {
        let p = &mut parts[i % n];
        if sum < total {
            let add = (total - sum).min(cap - *p);
            *p += add;
            sum += add;
        } else {
            let take = (sum - total).min(*p - MIN_SPLIT_ZATOSHIS);
            *p -= take;
            sum -= take;
        }
        i += 1;
        debug_assert!(i <= 2 * n, "split_with_cap failed to converge");
        if i > 2 * n {
            break;
        }
    }
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[test]
    fn split_with_cap_respects_bounds_and_total() {
        let mut rng = StdRng::seed_from_u64(42);
        for (total, cap) in [
            (300_000u64, 200_000u64),
            (1_000_001, 200_000),
            (5_000_000_000, 100_000_000),
            (200_001, 200_000),
            (999_999_999, 250_000),
        ] {
            let parts = split_with_cap(total, cap, &mut rng);
            assert_eq!(
                parts.iter().sum::<u64>(),
                total,
                "total={total} cap={cap} parts={parts:?}"
            );
            assert!(
                parts.iter().all(|&p| p >= MIN_SPLIT_ZATOSHIS && p <= cap),
                "total={total} cap={cap} parts={parts:?}"
            );
            assert!(parts.len() >= 2);
        }
    }

    #[test]
    fn split_with_cap_is_not_uniform() {
        let mut rng = StdRng::seed_from_u64(1);
        let parts = split_with_cap(1_000_000_000, 100_000_000, &mut rng);
        let first = parts[0];
        assert!(
            parts.iter().any(|&p| p != first),
            "randomized cap split produced equal parts: {:?}",
            parts
        );
    }
}
