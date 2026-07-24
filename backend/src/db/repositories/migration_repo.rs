use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::MySqlPool;

use crate::db::models_f4::{MigrationItem, MigrationRun};
use crate::error::AppResult;

pub struct MigrationRepository {
    pool: MySqlPool,
}

const RUN_COLS: &str = "id, source_wallet_id, mode, batch_count, window_hours, total_amount, \
    item_count, status, created_by_user_id, approved_by_user_id, reject_reason, \
    executed_by_user_id, executed_at, notes, created_at, updated_at";
const ITEM_COLS: &str = "id, run_id, seq, amount, scheduled_at, status, tx_hash, \
    error_message, retry_count, last_attempt_at, created_at, updated_at";

impl MigrationRepository {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }

    /// Insert run + planned batch items atomically. The run starts in
    /// `pending`; items carry their absolute schedule so a service restart
    /// loses nothing (the executor is fully DB-driven).
    pub async fn create_run_with_items(
        &self,
        source_wallet_id: i32,
        mode: &str,
        batch_count: u32,
        window_hours: u32,
        total_amount: Decimal,
        created_by_user_id: i32,
        notes: Option<&str>,
        items: &[(Decimal, Option<DateTime<Utc>>)],
    ) -> AppResult<i32> {
        let mut tx = self.pool.begin().await?;

        let run_result = sqlx::query(
            r#"INSERT INTO migration_runs
            (source_wallet_id, mode, batch_count, window_hours, total_amount,
             item_count, status, created_by_user_id, notes)
            VALUES (?, ?, ?, ?, ?, ?, 'pending', ?, ?)"#,
        )
        .bind(source_wallet_id)
        .bind(mode)
        .bind(batch_count as i32)
        .bind(window_hours as i32)
        .bind(total_amount)
        .bind(items.len() as i32)
        .bind(created_by_user_id)
        .bind(notes)
        .execute(&mut *tx)
        .await?;
        let run_id = run_result.last_insert_id() as i32;

        for (seq, (amount, scheduled_at)) in items.iter().enumerate() {
            sqlx::query(
                r#"INSERT INTO migration_items
                (run_id, seq, amount, scheduled_at, status)
                VALUES (?, ?, ?, ?, 'pending')"#,
            )
            .bind(run_id)
            .bind(seq as i32)
            .bind(amount)
            .bind(scheduled_at)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(run_id)
    }

    pub async fn find_run_by_id(&self, id: i32) -> AppResult<Option<MigrationRun>> {
        let sql = format!("SELECT {RUN_COLS} FROM migration_runs WHERE id = ?");
        Ok(sqlx::query_as::<_, MigrationRun>(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn list_runs(&self, limit: i32, offset: i32) -> AppResult<Vec<MigrationRun>> {
        let sql = format!(
            "SELECT {RUN_COLS} FROM migration_runs ORDER BY created_at DESC LIMIT ? OFFSET ?"
        );
        Ok(sqlx::query_as::<_, MigrationRun>(&sql)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?)
    }

    /// The single non-terminal run for a wallet, if any — used both by the
    /// wallet banner and to refuse overlapping migrations for one wallet.
    pub async fn find_active_run_for_wallet(
        &self,
        wallet_id: i32,
    ) -> AppResult<Option<MigrationRun>> {
        let sql = format!(
            "SELECT {RUN_COLS} FROM migration_runs
             WHERE source_wallet_id = ?
               AND status IN ('pending', 'awaiting_approval', 'approved', 'executing')
             ORDER BY id DESC LIMIT 1"
        );
        Ok(sqlx::query_as::<_, MigrationRun>(&sql)
            .bind(wallet_id)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn list_items_by_run(&self, run_id: i32) -> AppResult<Vec<MigrationItem>> {
        let sql = format!("SELECT {ITEM_COLS} FROM migration_items WHERE run_id = ? ORDER BY seq ASC");
        Ok(sqlx::query_as::<_, MigrationItem>(&sql)
            .bind(run_id)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn find_item(&self, run_id: i32, item_id: i32) -> AppResult<Option<MigrationItem>> {
        let sql = format!("SELECT {ITEM_COLS} FROM migration_items WHERE run_id = ? AND id = ?");
        Ok(sqlx::query_as::<_, MigrationItem>(&sql)
            .bind(run_id)
            .bind(item_id)
            .fetch_optional(&self.pool)
            .await?)
    }

    /// Due items across all `executing` runs, oldest schedule first.
    /// At most one item per run is returned so the executor never spends
    /// against the same wallet's note set twice in one tick (note selection
    /// only learns about spends after the previous transfer lands).
    pub async fn find_due_items(&self, limit: i32) -> AppResult<Vec<MigrationItem>> {
        let sql = format!(
            "SELECT {cols} FROM (
                SELECT i.*,
                       ROW_NUMBER() OVER (PARTITION BY i.run_id ORDER BY i.seq ASC) AS rn
                FROM migration_items i
                JOIN migration_runs r ON r.id = i.run_id
                WHERE r.status = 'executing'
                  AND i.status = 'pending'
                  AND (i.scheduled_at IS NULL OR i.scheduled_at <= NOW())
            ) ranked
            WHERE rn = 1
            ORDER BY scheduled_at IS NULL DESC, scheduled_at ASC
            LIMIT ?",
            cols = ITEM_COLS
        );
        Ok(sqlx::query_as::<_, MigrationItem>(&sql)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?)
    }

    /// Most recent submitted tx hashes for a wallet — the executor's
    /// double-spend gate checks these are mined before spending again
    /// (note selection only sees a spend after its block is scanned).
    pub async fn recent_submitted_tx_hashes_for_wallet(
        &self,
        wallet_id: i32,
        limit: i32,
    ) -> AppResult<Vec<String>> {
        let rows: Vec<(String,)> = sqlx::query_as(
            r#"SELECT i.tx_hash FROM migration_items i
               JOIN migration_runs r ON r.id = i.run_id
               WHERE r.source_wallet_id = ? AND i.status = 'submitted'
                 AND i.tx_hash IS NOT NULL
               ORDER BY i.last_attempt_at DESC LIMIT ?"#,
        )
        .bind(wallet_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(|(h,)| h).collect())
    }

    /// Runs in `executing` whose items have all reached a terminal state —
    /// candidates for final status folding.
    pub async fn find_finalizable_runs(&self) -> AppResult<Vec<MigrationRun>> {
        let sql = format!(
            "SELECT {RUN_COLS} FROM migration_runs r
             WHERE r.status = 'executing'
               AND NOT EXISTS (
                   SELECT 1 FROM migration_items i
                   WHERE i.run_id = r.id AND i.status = 'pending'
               )"
        );
        Ok(sqlx::query_as::<_, MigrationRun>(&sql)
            .fetch_all(&self.pool)
            .await?)
    }

    /// Status transitions are validated by the service layer (single caller);
    /// this just persists them, mirroring `PayrollRepository`.
    pub async fn update_run_status(
        &self,
        id: i32,
        status: &str,
        executed_by_user_id: Option<i32>,
        executed_at: Option<DateTime<Utc>>,
    ) -> AppResult<()> {
        sqlx::query(
            r#"UPDATE migration_runs
               SET status = ?, executed_by_user_id = COALESCE(?, executed_by_user_id),
                   executed_at = COALESCE(?, executed_at)
               WHERE id = ?"#,
        )
        .bind(status)
        .bind(executed_by_user_id)
        .bind(executed_at)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Maker ≠ checker is enforced in SQL, mirroring F2.1: the UPDATE simply
    /// does not match when the approver created the run, so a same-user
    /// approval cannot happen even if a service-layer check is bypassed.
    pub async fn approve_run(&self, id: i32, approver_user_id: i32) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE migration_runs
               SET status = 'approved', approved_by_user_id = ?
               WHERE id = ? AND status = 'awaiting_approval'
                 AND created_by_user_id <> ?"#,
        )
        .bind(approver_user_id)
        .bind(id)
        .bind(approver_user_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn reject_run(
        &self,
        id: i32,
        approver_user_id: i32,
        reason: &str,
    ) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE migration_runs
               SET status = 'rejected', approved_by_user_id = ?, reject_reason = ?
               WHERE id = ? AND status = 'awaiting_approval'
                 AND created_by_user_id <> ?"#,
        )
        .bind(approver_user_id)
        .bind(reason)
        .bind(id)
        .bind(approver_user_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn mark_item_submitted(&self, item_id: i32, tx_hash: &str) -> AppResult<()> {
        sqlx::query(
            r#"UPDATE migration_items
               SET status = 'submitted', tx_hash = ?, error_message = NULL,
                   retry_count = retry_count + 1, last_attempt_at = CURRENT_TIMESTAMP
               WHERE id = ?"#,
        )
        .bind(tx_hash)
        .bind(item_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_item_failed(&self, item_id: i32, error_message: &str) -> AppResult<()> {
        sqlx::query(
            r#"UPDATE migration_items
               SET status = 'failed', error_message = ?,
                   retry_count = retry_count + 1, last_attempt_at = CURRENT_TIMESTAMP
               WHERE id = ?"#,
        )
        .bind(error_message)
        .bind(item_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Reset a failed item for another attempt by the executor.
    pub async fn reset_item_for_retry(&self, item_id: i32) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE migration_items
               SET status = 'pending', error_message = NULL
               WHERE id = ? AND status = 'failed'"#,
        )
        .bind(item_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Cancel every item that has not yet been attempted. Items already
    /// submitted on-chain cannot be reversed and keep their status.
    pub async fn cancel_pending_items(&self, run_id: i32) -> AppResult<u64> {
        let result = sqlx::query(
            r#"UPDATE migration_items
               SET status = 'canceled'
               WHERE run_id = ? AND status = 'pending'"#,
        )
        .bind(run_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    /// COUNT(CASE …) keeps BIGINT decoding (see `PayrollRepository` note on
    /// MySQL SUM returning DECIMAL).
    pub async fn count_items_by_status(&self, run_id: i32) -> AppResult<MigrationItemCounts> {
        let row: (i64, i64, i64, i64, i64) = sqlx::query_as(
            r#"SELECT
                 COUNT(CASE WHEN status = 'pending'   THEN 1 END) AS pending,
                 COUNT(CASE WHEN status = 'submitted' THEN 1 END) AS submitted,
                 COUNT(CASE WHEN status = 'failed'    THEN 1 END) AS failed,
                 COUNT(CASE WHEN status = 'canceled'  THEN 1 END) AS canceled,
                 COUNT(*)                                          AS total
               FROM migration_items WHERE run_id = ?"#,
        )
        .bind(run_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(MigrationItemCounts {
            pending: row.0,
            submitted: row.1,
            failed: row.2,
            canceled: row.3,
            total: row.4,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MigrationItemCounts {
    pub pending: i64,
    pub submitted: i64,
    pub failed: i64,
    pub canceled: i64,
    pub total: i64,
}
