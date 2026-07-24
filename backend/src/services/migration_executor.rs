//! F4.1 — background executor for migration runs.
//!
//! Fully DB-driven: every tick it picks due `pending` items of `executing`
//! runs (at most one per run, so note selection never races itself on a
//! wallet) and pushes each through the wallet service's real
//! privacy-transfer path — proposal, proof, broadcast — with the wallet's
//! own unified address as recipient. Nothing lives in memory, so a service
//! restart resumes exactly where the tables say we are.
//!
//! Failure policy (PRD-F4 §6, no-mock rule): any error — RPC down, proof
//! failure, insufficient funds — lands verbatim in `error_message`, the item
//! goes to `failed`, and siblings continue. When no `pending` items remain,
//! the run folds to completed / partial / failed.

use std::sync::Arc;
use std::time::Duration;

use crate::db::models_f4::MigrationItem;
use crate::db::repositories::{MigrationRepository, WalletRepository};
use crate::error::{AppError, AppResult};
use crate::services::migration_service::zec_to_zatoshis;
use crate::services::WalletService;

/// How many due items to process per tick across all runs.
const TICK_BATCH_LIMIT: i32 = 8;

pub struct MigrationExecutor {
    repo: MigrationRepository,
    wallet_repo: WalletRepository,
    wallet_service: Arc<WalletService>,
    tick_interval: Duration,
}

impl MigrationExecutor {
    pub fn new(
        repo: MigrationRepository,
        wallet_repo: WalletRepository,
        wallet_service: Arc<WalletService>,
        tick_interval_secs: u64,
    ) -> Self {
        Self {
            repo,
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
                "[migration-executor] started (tick every {:?})",
                self.tick_interval
            );
            loop {
                if let Err(e) = self.tick().await {
                    tracing::error!("[migration-executor] tick failed: {}", e);
                }
                tokio::time::sleep(self.tick_interval).await;
            }
        })
    }

    async fn tick(&self) -> AppResult<()> {
        let due = self.repo.find_due_items(TICK_BATCH_LIMIT).await?;
        for item in due {
            // Items run sequentially: proofs are CPU-heavy and two proposals
            // against the same wallet in flight would double-select notes.
            if let Err(e) = self.execute_item(&item).await {
                let msg = e.to_string();
                tracing::warn!(
                    "[migration-executor] run {} item {} (seq {}) failed: {}",
                    item.run_id,
                    item.id,
                    item.seq,
                    msg
                );
                self.repo.mark_item_failed(item.id, &msg).await?;
            }
        }
        self.finalize_runs().await
    }

    /// One batch: self-transfer `item.amount` of shielded funds. Recipient is
    /// the wallet's own unified address — pre-NU6.3 an Orchard-pool
    /// self-transfer, after F4.0's height-aware builder the same call is the
    /// turnstile crossing into Ironwood. No memo: a constant memo would be a
    /// linkable fingerprint on the turnstile.
    async fn execute_item(&self, item: &MigrationItem) -> AppResult<()> {
        let run = self
            .repo
            .find_run_by_id(item.run_id)
            .await?
            .ok_or_else(|| {
                AppError::InternalError(format!("run {} vanished mid-execution", item.run_id))
            })?;
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
                "[migration-executor] run {} item {} clamped {} -> {} zatoshis (fee drift)",
                run.id,
                item.id,
                planned_zat,
                amount_zat
            );
        }

        let amount_zec = format!("{}", zatoshis_display(amount_zat));
        let proposal = self
            .wallet_service
            .create_privacy_transfer_proposal(
                wallet.id,
                &self_address,
                &amount_zec,
                Some(amount_zat),
                None,
                crate::blockchain::zcash::orchard::transfer::FundSource::Shielded,
            )
            .await?;

        let result = self
            .wallet_service
            .execute_privacy_transfer(wallet.id, &proposal)
            .await?;

        tracing::info!(
            "[migration-executor] run {} item {} (seq {}) submitted: tx={} amount={} fee={}",
            run.id,
            item.id,
            item.seq,
            result.tx_id,
            result.amount_zatoshis,
            result.fee_zatoshis
        );
        self.repo.mark_item_submitted(item.id, &result.tx_id).await
    }

    /// Fold `executing` runs whose items are all terminal into their final
    /// status: completed (all submitted), failed (nothing submitted),
    /// partial otherwise (canceled-only runs were already folded by cancel).
    async fn finalize_runs(&self) -> AppResult<()> {
        for run in self.repo.find_finalizable_runs().await? {
            let counts = self.repo.count_items_by_status(run.id).await?;
            let final_status = if counts.failed == 0 && counts.submitted == counts.total {
                "completed"
            } else if counts.submitted > 0 {
                "partial"
            } else {
                "failed"
            };
            self.repo
                .update_run_status(run.id, final_status, None, None)
                .await?;
            tracing::info!(
                "[migration-executor] run {} folded -> {} (submitted={} failed={} canceled={})",
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

/// Render zatoshis as a plain-decimal ZEC string for the proposal API
/// (which parses it as f64 only when explicit zatoshis are absent — we
/// always pass zatoshis, the string is informational).
fn zatoshis_display(zat: u64) -> String {
    format!("{}.{:08}", zat / 100_000_000, zat % 100_000_000)
}

#[cfg(test)]
mod tests {
    use super::zatoshis_display;

    #[test]
    fn zatoshis_display_is_plain_decimal() {
        assert_eq!(zatoshis_display(0), "0.00000000");
        assert_eq!(zatoshis_display(1), "0.00000001");
        assert_eq!(zatoshis_display(123_456_789), "1.23456789");
        assert_eq!(zatoshis_display(2_100_000_000_000_000), "21000000.00000000");
    }
}
