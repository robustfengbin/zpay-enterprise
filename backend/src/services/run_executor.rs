//! F4 — background executor shared by migration runs (F4.1) and batch
//! privacy transfer runs (F4.2). One executor, two run types (PRD F4.2.5):
//! both flow through the wallet service's real privacy-transfer path —
//! proposal, proof, broadcast — and both are fully DB-driven, so a service
//! restart resumes exactly where the tables say we are.
//!
//! Concurrency discipline: note selection only learns about a spend after
//! the previous transfer lands, so the executor runs items sequentially
//! and touches each *wallet* at most once per tick — across both run
//! types. Migration items go first: the turnstile migration is the
//! survival path.
//!
//! Failure policy (PRD-F4 §6, no-mock rule): any error — RPC down, proof
//! failure, insufficient funds — lands verbatim in `error_message`, the
//! item goes to `failed`, and siblings continue. When no `pending` items
//! remain, the run folds to completed / partial / failed.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use crate::blockchain::traits::TxStatus;
use crate::db::models_f4::{BatchTransferItem, MigrationItem};
use crate::db::repositories::{BatchTransferRepository, MigrationRepository, WalletRepository};
use crate::error::{AppError, AppResult};
use crate::services::migration_service::zec_to_zatoshis;
use crate::services::WalletService;

/// How many due items to consider per tick and per run type.
const TICK_BATCH_LIMIT: i32 = 8;

pub struct RunExecutor {
    migration_repo: MigrationRepository,
    batch_repo: BatchTransferRepository,
    wallet_repo: WalletRepository,
    wallet_service: Arc<WalletService>,
    tick_interval: Duration,
}

impl RunExecutor {
    pub fn new(
        migration_repo: MigrationRepository,
        batch_repo: BatchTransferRepository,
        wallet_repo: WalletRepository,
        wallet_service: Arc<WalletService>,
        tick_interval_secs: u64,
    ) -> Self {
        Self {
            migration_repo,
            batch_repo,
            wallet_repo,
            wallet_service,
            tick_interval: Duration::from_secs(tick_interval_secs.max(5)),
        }
    }

    /// Spawn the executor loop. Errors inside a tick are logged and never
    /// kill the loop; the next tick re-reads the tables.
    pub fn spawn(self) {
        // Executor ticks call wallet sync (double-spend guard) and drive real
        // transfer builds — heavy work that must stay off the main runtime
        // (see services::sync_runtime).
        crate::services::sync_runtime::spawn_loop(async move {
            tracing::info!(
                "[run-executor] started (tick every {:?})",
                self.tick_interval
            );
            loop {
                if let Err(e) = self.tick().await {
                    tracing::error!("[run-executor] tick failed: {}", e);
                }
                tokio::time::sleep(self.tick_interval).await;
            }
        });
    }

    async fn tick(&self) -> AppResult<()> {
        // Wallets already spent against this tick; an item whose wallet is
        // taken stays `pending` and the next tick picks it up.
        let mut touched_wallets: HashSet<i32> = HashSet::new();

        let due_migrations = self.migration_repo.find_due_items(TICK_BATCH_LIMIT).await?;
        let due_batches = self.batch_repo.find_due_items(TICK_BATCH_LIMIT).await?;
        if due_migrations.is_empty() && due_batches.is_empty() {
            return self.finalize_runs().await;
        }

        // Scan first so notes spent by already-mined broadcasts are marked
        // before note selection runs (half of the double-spend gate; the
        // other half is wallet_has_unmined_spend below).
        if let Err(e) = self.wallet_service.sync_orchard().await {
            tracing::warn!("[run-executor] pre-spend scan sync failed: {}", e);
        }

        for item in due_migrations {
            if let Err(e) = self.execute_migration_item(&item, &mut touched_wallets).await {
                let msg = e.to_string();
                tracing::warn!(
                    "[run-executor] migration run {} item {} (seq {}) failed: {}",
                    item.run_id,
                    item.id,
                    item.seq,
                    msg
                );
                self.migration_repo.mark_item_failed(item.id, &msg).await?;
            }
        }

        for item in due_batches {
            if let Err(e) = self.execute_batch_item(&item, &mut touched_wallets).await {
                let msg = e.to_string();
                tracing::warn!(
                    "[run-executor] batch run {} item {} (seq {}) failed: {}",
                    item.run_id,
                    item.id,
                    item.seq,
                    msg
                );
                self.batch_repo.mark_item_failed(item.id, &msg).await?;
            }
        }

        self.finalize_runs().await
    }

    /// One migration batch: self-transfer `item.amount` of shielded funds.
    /// Recipient is the wallet's own unified address — pre-NU6.3 an
    /// Orchard-pool self-transfer, after F4.0's height-aware builder the
    /// same call is the turnstile crossing into Ironwood. No memo: a
    /// constant memo would be a linkable fingerprint on the turnstile.
    /// Returns Ok(false) if the item was skipped for wallet serialization.
    async fn execute_migration_item(
        &self,
        item: &MigrationItem,
        touched_wallets: &mut HashSet<i32>,
    ) -> AppResult<bool> {
        let run = self
            .migration_repo
            .find_run_by_id(item.run_id)
            .await?
            .ok_or_else(|| {
                AppError::InternalError(format!("run {} vanished mid-execution", item.run_id))
            })?;
        if !touched_wallets.insert(run.source_wallet_id) {
            return Ok(false);
        }
        if self.wallet_has_unmined_spend(run.source_wallet_id).await? {
            tracing::info!(
                "[run-executor] wallet {} has an unmined spend; migration item {} waits",
                run.source_wallet_id,
                item.id
            );
            return Ok(false);
        }
        let wallet = self
            .wallet_repo
            .find_by_id(run.source_wallet_id)
            .await?
            .ok_or_else(|| {
                AppError::InternalError(format!(
                    "source_wallet {} vanished mid-execution",
                    run.source_wallet_id
                ))
            })?;

        let addresses = self.wallet_service.get_unified_addresses(wallet.id).await?;
        let self_address = addresses
            .iter()
            .find(|a| a.has_orchard)
            .map(|a| a.address.clone())
            .ok_or_else(|| {
                AppError::ValidationError(format!(
                    "wallet {} has no Orchard-capable unified address; enable Orchard first",
                    wallet.id
                ))
            })?;

        let planned_zat = zec_to_zatoshis(item.amount)?;

        // Clamp against the live balance: earlier batches paid fees out of
        // the plan's headroom, so the final batch sweeps what is actually
        // left instead of failing on a few thousand zatoshis of drift.
        // (Migration-only: a batch *payment* must never be silently
        // shrunk — see execute_batch_item.)
        // Old pool only: a migration moves what is still behind the turnstile.
        // The cross-pool total would count funds this run already delivered
        // into Ironwood, so the final batch would try to "sweep" them back
        // through the turnstile — which the builder refuses by design.
        let legacy_spendable = self.legacy_pool_spendable(wallet.id).await?;
        let fee_reserve = crate::blockchain::zcash::orchard::constants::DEFAULT_FEE_ZATOSHIS;
        let available = legacy_spendable.saturating_sub(fee_reserve);
        if available == 0 {
            return Err(AppError::InsufficientBalance(format!(
                "no spendable legacy-pool balance left (balance {} zatoshis, fee reserve {})",
                legacy_spendable, fee_reserve
            )));
        }
        let amount_zat = planned_zat.min(available);
        if amount_zat < planned_zat {
            tracing::info!(
                "[run-executor] migration run {} item {} clamped {} -> {} zatoshis (fee drift)",
                run.id,
                item.id,
                planned_zat,
                amount_zat
            );
        }

        // Known issue (loss-free): if the process dies between the broadcast
        // inside submit_privacy_transfer and mark_item_submitted below, resume
        // re-selects the same notes and the node rejects the duplicate spend
        // (-25), failing the item; a manual per-item retry after the original
        // tx mines (and a rescan marks the notes spent) recovers it. Funds are
        // never double-spent or lost — the wallet_has_unmined_spend guard and
        // the node's nullifier check both hold.
        // Non-final batches keep their change in the old pool so the batches
        // still to come have notes left to migrate; the last one lets
        // everything cross.
        let tx_id = self
            .submit_privacy_transfer(
                wallet.id,
                &self_address,
                amount_zat,
                None,
                keeps_change_in_old_pool(item.seq, run.item_count),
            )
            .await?;
        tracing::info!(
            "[run-executor] migration run {} item {} (seq {}) submitted: tx={} amount={}",
            run.id,
            item.id,
            item.seq,
            tx_id,
            amount_zat
        );
        self.migration_repo
            .mark_item_submitted(item.id, &tx_id)
            .await?;
        Ok(true)
    }

    /// One batch-transfer payment to a third-party recipient. Unlike
    /// migration self-transfers there is NO amount clamping: silently
    /// shrinking a payment would underpay the recipient. If the balance
    /// can't cover it, the proposal fails and the real error lands on the
    /// item. Returns Ok(false) if skipped for wallet serialization.
    async fn execute_batch_item(
        &self,
        item: &BatchTransferItem,
        touched_wallets: &mut HashSet<i32>,
    ) -> AppResult<bool> {
        let run = self
            .batch_repo
            .find_run_by_id(item.run_id)
            .await?
            .ok_or_else(|| {
                AppError::InternalError(format!("run {} vanished mid-execution", item.run_id))
            })?;
        if !touched_wallets.insert(run.source_wallet_id) {
            return Ok(false);
        }
        if self.wallet_has_unmined_spend(run.source_wallet_id).await? {
            tracing::info!(
                "[run-executor] wallet {} has an unmined spend; batch item {} waits",
                run.source_wallet_id,
                item.id
            );
            return Ok(false);
        }

        let amount_zat = zec_to_zatoshis(item.amount)?;
        // Same rule as a migration batch: while the run is still funded from
        // the old pool, every payment but the last leaves its change there so
        // the remaining payments can still be made from it.
        let tx_id = self
            .submit_privacy_transfer(
                run.source_wallet_id,
                &item.recipient_address,
                amount_zat,
                item.memo.clone(),
                keeps_change_in_old_pool(item.seq, run.item_count),
            )
            .await?;
        tracing::info!(
            "[run-executor] batch run {} item {} (seq {}) submitted: tx={} amount={}",
            run.id,
            item.id,
            item.seq,
            tx_id,
            amount_zat
        );
        self.batch_repo.mark_item_submitted(item.id, &tx_id).await?;
        Ok(true)
    }

    /// Double-spend gate, half two: true while any recent broadcast from
    /// this wallet is still unmined. The note DB only marks a note spent
    /// once the containing block is scanned, so spending before then would
    /// re-select the same notes and the node rejects the duplicate
    /// nullifier (RPC -25 — hit in e2e 07-24). Fail-closed: if the node
    /// can't confirm status, we wait a tick rather than risk a double-spend.
    async fn wallet_has_unmined_spend(&self, wallet_id: i32) -> AppResult<bool> {
        let mut txs = self
            .migration_repo
            .recent_submitted_tx_hashes_for_wallet(wallet_id, 3)
            .await?;
        txs.extend(
            self.batch_repo
                .recent_submitted_tx_hashes_for_wallet(wallet_id, 3)
                .await?,
        );
        for tx in txs {
            match self.wallet_service.zcash_tx_status(&tx).await {
                Ok(TxStatus::Pending) => return Ok(true),
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!(
                        "[run-executor] tx status check failed for {} (wallet {}): {} — waiting",
                        tx,
                        wallet_id,
                        e
                    );
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Spendable balance of the old Orchard pool alone.
    async fn legacy_pool_spendable(&self, wallet_id: i32) -> AppResult<u64> {
        use crate::blockchain::zcash::orchard::ShieldedPool;

        let by_pool = self
            .wallet_service
            .get_shielded_balances_by_pool(wallet_id)
            .await?;
        Ok(by_pool
            .iter()
            .find(|b| b.pool == Some(ShieldedPool::Orchard))
            .map(|b| b.spendable_zatoshis)
            .unwrap_or(0))
    }

    /// Shared on-chain leg: proposal → proof → broadcast via the wallet
    /// service's real privacy-transfer path (the F4.0 dual-pool builder
    /// slots in underneath with zero changes here).
    async fn submit_privacy_transfer(
        &self,
        wallet_id: i32,
        to_address: &str,
        amount_zat: u64,
        memo: Option<String>,
        keep_change_in_old_pool: bool,
    ) -> AppResult<String> {
        use crate::blockchain::zcash::orchard::transfer::ChangePolicy;

        let amount_zec = zatoshis_display(amount_zat);
        let mut proposal = self
            .wallet_service
            .create_privacy_transfer_proposal(
                wallet_id,
                to_address,
                &amount_zec,
                Some(amount_zat),
                memo,
                crate::blockchain::zcash::orchard::transfer::FundSource::Shielded,
            )
            .await?;

        // Where the change of a post-NU6.3 turnstile spend lands decides
        // whether staggering survives: if a non-final batch let its change
        // cross into Ironwood with the payment, the old pool would be empty
        // and every later batch would have nothing left to move — the run
        // would collapse into the single transfer the private mode exists to
        // avoid. Ignored on the pre-NU6.3 and Ironwood-native paths, where
        // there is only one pool in play.
        if keep_change_in_old_pool {
            proposal.change_policy = ChangePolicy::KeepInOldPool;
        }

        let result = self
            .wallet_service
            .execute_privacy_transfer(wallet_id, &proposal)
            .await?;
        Ok(result.tx_id)
    }

    /// Fold `executing` runs whose items are all terminal into their final
    /// status: completed (all submitted), failed (nothing submitted),
    /// partial otherwise (canceled-only runs were already folded by cancel).
    async fn finalize_runs(&self) -> AppResult<()> {
        for run in self.migration_repo.find_finalizable_runs().await? {
            let counts = self.migration_repo.count_items_by_status(run.id).await?;
            let final_status = fold_status(counts.failed, counts.submitted, counts.total);
            self.migration_repo
                .update_run_status(run.id, final_status, None, None)
                .await?;
            tracing::info!(
                "[run-executor] migration run {} folded -> {} (submitted={} failed={} canceled={})",
                run.id,
                final_status,
                counts.submitted,
                counts.failed,
                counts.canceled
            );
        }
        for run in self.batch_repo.find_finalizable_runs().await? {
            let counts = self.batch_repo.count_items_by_status(run.id).await?;
            let final_status = fold_status(counts.failed, counts.submitted, counts.total);
            self.batch_repo
                .update_run_status(run.id, final_status, None, None)
                .await?;
            tracing::info!(
                "[run-executor] batch run {} folded -> {} (submitted={} failed={} canceled={})",
                run.id,
                final_status,
                counts.submitted,
                counts.failed,
                counts.canceled
            );
        }
        Ok(())
    }
}

/// Whether this item's change must stay in the old Orchard pool.
///
/// Every batch but the last: the point of a staggered run is that value
/// leaves over a window, and that only works while the old pool still holds
/// notes for the batches that have not run yet. The final batch lets the
/// remainder cross. Single-item runs (immediate migration) are "final" and
/// cross straight away.
fn keeps_change_in_old_pool(seq: i32, item_count: i32) -> bool {
    seq + 1 < item_count
}

fn fold_status(failed: i64, submitted: i64, total: i64) -> &'static str {
    if failed == 0 && submitted == total {
        "completed"
    } else if submitted > 0 {
        "partial"
    } else {
        "failed"
    }
}

/// Render zatoshis as a plain-decimal ZEC string for the proposal API
/// (which parses it as f64 only when explicit zatoshis are absent — we
/// always pass zatoshis, the string is informational).
fn zatoshis_display(zat: u64) -> String {
    format!("{}.{:08}", zat / 100_000_000, zat % 100_000_000)
}

#[cfg(test)]
mod tests {
    use super::{fold_status, keeps_change_in_old_pool, zatoshis_display};

    #[test]
    fn zatoshis_display_is_plain_decimal() {
        assert_eq!(zatoshis_display(0), "0.00000000");
        assert_eq!(zatoshis_display(1), "0.00000001");
        assert_eq!(zatoshis_display(123_456_789), "1.23456789");
        assert_eq!(zatoshis_display(2_100_000_000_000_000), "21000000.00000000");
    }

    #[test]
    fn only_the_last_batch_lets_change_cross() {
        // 3-batch private run: the first two retain, the last crosses.
        assert!(keeps_change_in_old_pool(0, 3));
        assert!(keeps_change_in_old_pool(1, 3));
        assert!(!keeps_change_in_old_pool(2, 3));
        // Immediate migration / single payment: nothing to stagger.
        assert!(!keeps_change_in_old_pool(0, 1));
    }

    #[test]
    fn fold_status_covers_all_outcomes() {
        assert_eq!(fold_status(0, 5, 5), "completed");
        assert_eq!(fold_status(1, 4, 5), "partial");
        assert_eq!(fold_status(5, 0, 5), "failed");
        // all canceled, nothing submitted
        assert_eq!(fold_status(0, 0, 5), "failed");
    }
}
