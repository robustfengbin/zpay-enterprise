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
    pub fn spawn(self) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
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
        })
    }

    async fn tick(&self) -> AppResult<()> {
        // Wallets already spent against this tick; an item whose wallet is
        // taken stays `pending` and the next tick picks it up.
        let mut touched_wallets: HashSet<i32> = HashSet::new();

        for item in self.migration_repo.find_due_items(TICK_BATCH_LIMIT).await? {
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

        for item in self.batch_repo.find_due_items(TICK_BATCH_LIMIT).await? {
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
        let balance = self.wallet_service.get_shielded_balance(wallet.id).await?;
        let fee_reserve = crate::blockchain::zcash::orchard::constants::DEFAULT_FEE_ZATOSHIS;
        let available = balance.spendable_zatoshis.saturating_sub(fee_reserve);
        if available == 0 {
            return Err(AppError::InsufficientBalance(format!(
                "no spendable shielded balance left (balance {} zatoshis, fee reserve {})",
                balance.spendable_zatoshis, fee_reserve
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

        let tx_id = self
            .submit_privacy_transfer(wallet.id, &self_address, amount_zat, None)
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

        let amount_zat = zec_to_zatoshis(item.amount)?;
        let tx_id = self
            .submit_privacy_transfer(
                run.source_wallet_id,
                &item.recipient_address,
                amount_zat,
                item.memo.clone(),
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

    /// Shared on-chain leg: proposal → proof → broadcast via the wallet
    /// service's real privacy-transfer path (the F4.0 dual-pool builder
    /// slots in underneath with zero changes here).
    async fn submit_privacy_transfer(
        &self,
        wallet_id: i32,
        to_address: &str,
        amount_zat: u64,
        memo: Option<String>,
    ) -> AppResult<String> {
        let amount_zec = zatoshis_display(amount_zat);
        let proposal = self
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
    use super::{fold_status, zatoshis_display};

    #[test]
    fn zatoshis_display_is_plain_decimal() {
        assert_eq!(zatoshis_display(0), "0.00000000");
        assert_eq!(zatoshis_display(1), "0.00000001");
        assert_eq!(zatoshis_display(123_456_789), "1.23456789");
        assert_eq!(zatoshis_display(2_100_000_000_000_000), "21000000.00000000");
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
