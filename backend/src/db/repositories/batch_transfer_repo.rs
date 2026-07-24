//! F4.2 — persistence for generic batch privacy transfer runs. Mirrors
//! `MigrationRepository` deliberately: the two run types share the executor
//! (PRD F4.2.5), so their state machines must stay column-for-column
//! comparable.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::MySqlPool;

use crate::db::models_f4::{BatchTransferItem, BatchTransferRun};
use crate::error::AppResult;

pub struct BatchTransferRepository {
    pool: MySqlPool,
}

const RUN_COLS: &str = "id, title, source_wallet_id, privacy_mode, batch_count, window_hours, \
    total_amount, item_count, status, created_by_user_id, approved_by_user_id, reject_reason, \
    executed_by_user_id, executed_at, notes, created_at, updated_at";
const ITEM_COLS: &str = "id, run_id, seq, recipient_address, amount, memo, scheduled_at, status, \
    tx_hash, error_message, retry_count, last_attempt_at, created_at, updated_at";

/// Planned item shape produced by the service: recipient, amount, memo,
/// absolute schedule slot (None = eligible immediately).
pub type PlannedBatchItem = (String, Decimal, Option<String>, Option<DateTime<Utc>>);

impl BatchTransferRepository {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }

    pub async fn create_run_with_items(
        &self,
        title: &str,
        source_wallet_id: i32,
        privacy_mode: &str,
        batch_count: u32,
        window_hours: u32,
        total_amount: Decimal,
        created_by_user_id: i32,
        notes: Option<&str>,
        items: &[PlannedBatchItem],
    ) -> AppResult<i32> {
        let mut tx = self.pool.begin().await?;

        let run_result = sqlx::query(
            r#"INSERT INTO batch_transfer_runs
            (title, source_wallet_id, privacy_mode, batch_count, window_hours,
             total_amount, item_count, status, created_by_user_id, notes)
            VALUES (?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?)"#,
        )
        .bind(title)
        .bind(source_wallet_id)
        .bind(privacy_mode)
        .bind(batch_count as i32)
        .bind(window_hours as i32)
        .bind(total_amount)
        .bind(items.len() as i32)
        .bind(created_by_user_id)
        .bind(notes)
        .execute(&mut *tx)
        .await?;
        let run_id = run_result.last_insert_id() as i32;

        for (seq, (recipient, amount, memo, scheduled_at)) in items.iter().enumerate() {
            sqlx::query(
                r#"INSERT INTO batch_transfer_items
                (run_id, seq, recipient_address, amount, memo, scheduled_at, status)
                VALUES (?, ?, ?, ?, ?, ?, 'pending')"#,
            )
            .bind(run_id)
            .bind(seq as i32)
            .bind(recipient)
            .bind(amount)
            .bind(memo)
            .bind(scheduled_at)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(run_id)
    }

    pub async fn find_run_by_id(&self, id: i32) -> AppResult<Option<BatchTransferRun>> {
        let sql = format!("SELECT {RUN_COLS} FROM batch_transfer_runs WHERE id = ?");
        Ok(sqlx::query_as::<_, BatchTransferRun>(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn list_runs(&self, limit: i32, offset: i32) -> AppResult<Vec<BatchTransferRun>> {
        let sql = format!(
            "SELECT {RUN_COLS} FROM batch_transfer_runs ORDER BY created_at DESC LIMIT ? OFFSET ?"
        );
        Ok(sqlx::query_as::<_, BatchTransferRun>(&sql)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?)
    }

    /// Non-terminal run for a wallet. Unlike migrations (whole-treasury,
    /// one at a time), batch runs may legitimately queue up — this exists
    /// for the executor's wallet-serialization and for UI hints, not to
    /// refuse creation.
    pub async fn find_active_run_for_wallet(
        &self,
        wallet_id: i32,
    ) -> AppResult<Option<BatchTransferRun>> {
        let sql = format!(
            "SELECT {RUN_COLS} FROM batch_transfer_runs
             WHERE source_wallet_id = ?
               AND status IN ('pending', 'awaiting_approval', 'approved', 'executing')
             ORDER BY id DESC LIMIT 1"
        );
        Ok(sqlx::query_as::<_, BatchTransferRun>(&sql)
            .bind(wallet_id)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn list_items_by_run(&self, run_id: i32) -> AppResult<Vec<BatchTransferItem>> {
        let sql =
            format!("SELECT {ITEM_COLS} FROM batch_transfer_items WHERE run_id = ? ORDER BY seq ASC");
        Ok(sqlx::query_as::<_, BatchTransferItem>(&sql)
            .bind(run_id)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn find_item(
        &self,
        run_id: i32,
        item_id: i32,
    ) -> AppResult<Option<BatchTransferItem>> {
        let sql = format!("SELECT {ITEM_COLS} FROM batch_transfer_items WHERE run_id = ? AND id = ?");
        Ok(sqlx::query_as::<_, BatchTransferItem>(&sql)
            .bind(run_id)
            .bind(item_id)
            .fetch_optional(&self.pool)
            .await?)
    }

    /// Due items across all `executing` runs, at most one per run per tick —
    /// same note-selection discipline as migrations (see
    /// `MigrationRepository::find_due_items`).
    pub async fn find_due_items(&self, limit: i32) -> AppResult<Vec<BatchTransferItem>> {
        let sql = format!(
            "SELECT {cols} FROM (
                SELECT i.*,
                       ROW_NUMBER() OVER (PARTITION BY i.run_id ORDER BY i.seq ASC) AS rn
                FROM batch_transfer_items i
                JOIN batch_transfer_runs r ON r.id = i.run_id
                WHERE r.status = 'executing'
                  AND i.status = 'pending'
                  AND (i.scheduled_at IS NULL OR i.scheduled_at <= NOW())
            ) ranked
            WHERE rn = 1
            ORDER BY scheduled_at IS NULL DESC, scheduled_at ASC
            LIMIT ?",
            cols = ITEM_COLS
        );
        Ok(sqlx::query_as::<_, BatchTransferItem>(&sql)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn find_finalizable_runs(&self) -> AppResult<Vec<BatchTransferRun>> {
        let sql = format!(
            "SELECT {RUN_COLS} FROM batch_transfer_runs r
             WHERE r.status = 'executing'
               AND NOT EXISTS (
                   SELECT 1 FROM batch_transfer_items i
                   WHERE i.run_id = r.id AND i.status = 'pending'
               )"
        );
        Ok(sqlx::query_as::<_, BatchTransferRun>(&sql)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn update_run_status(
        &self,
        id: i32,
        status: &str,
        executed_by_user_id: Option<i32>,
        executed_at: Option<DateTime<Utc>>,
    ) -> AppResult<()> {
        sqlx::query(
            r#"UPDATE batch_transfer_runs
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

    /// Maker ≠ checker enforced in SQL, same as migrations / F2.1.
    pub async fn approve_run(&self, id: i32, approver_user_id: i32) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE batch_transfer_runs
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
            r#"UPDATE batch_transfer_runs
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
            r#"UPDATE batch_transfer_items
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
            r#"UPDATE batch_transfer_items
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

    pub async fn reset_item_for_retry(&self, item_id: i32) -> AppResult<bool> {
        let result = sqlx::query(
            r#"UPDATE batch_transfer_items
               SET status = 'pending', error_message = NULL
               WHERE id = ? AND status = 'failed'"#,
        )
        .bind(item_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn cancel_pending_items(&self, run_id: i32) -> AppResult<u64> {
        let result = sqlx::query(
            r#"UPDATE batch_transfer_items
               SET status = 'canceled'
               WHERE run_id = ? AND status = 'pending'"#,
        )
        .bind(run_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn count_items_by_status(&self, run_id: i32) -> AppResult<BatchTransferItemCounts> {
        let row: (i64, i64, i64, i64, i64) = sqlx::query_as(
            r#"SELECT
                 COUNT(CASE WHEN status = 'pending'   THEN 1 END) AS pending,
                 COUNT(CASE WHEN status = 'submitted' THEN 1 END) AS submitted,
                 COUNT(CASE WHEN status = 'failed'    THEN 1 END) AS failed,
                 COUNT(CASE WHEN status = 'canceled'  THEN 1 END) AS canceled,
                 COUNT(*)                                          AS total
               FROM batch_transfer_items WHERE run_id = ?"#,
        )
        .bind(run_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(BatchTransferItemCounts {
            pending: row.0,
            submitted: row.1,
            failed: row.2,
            canceled: row.3,
            total: row.4,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BatchTransferItemCounts {
    pub pending: i64,
    pub submitted: i64,
    pub failed: i64,
    pub canceled: i64,
    pub total: i64,
}
