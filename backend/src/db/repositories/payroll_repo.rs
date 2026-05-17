use crate::db::models_m1::{PayrollItem, PayrollItemInput, PayrollRun};
use crate::error::AppResult;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::MySqlPool;

pub struct PayrollRepository {
    pool: MySqlPool,
}

const RUN_COLS: &str = "id, pay_period, source_wallet_id, total_amount, item_count, status, \
    created_by_user_id, executed_by_user_id, executed_at, notes, created_at, updated_at";
const ITEM_COLS: &str = "id, run_id, employee_id, employee_address, amount, memo, status, \
    tx_hash, block_number, transfer_id, error_message, retry_count, last_attempt_at, \
    created_at, updated_at";

impl PayrollRepository {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }

    /// Insert run + items atomically. The run starts in `pending` and is the
    /// only legal state from which `execute` may transition.
    pub async fn create_run_with_items(
        &self,
        pay_period: &str,
        source_wallet_id: i32,
        total_amount: Decimal,
        created_by_user_id: i32,
        notes: Option<&str>,
        items: &[(Option<i32>, &PayrollItemInput, Decimal)],
    ) -> AppResult<i32> {
        let mut tx = self.pool.begin().await?;

        let run_result = sqlx::query(
            r#"INSERT INTO payroll_runs
            (pay_period, source_wallet_id, total_amount, item_count, status, created_by_user_id, notes)
            VALUES (?, ?, ?, ?, 'pending', ?, ?)"#,
        )
        .bind(pay_period)
        .bind(source_wallet_id)
        .bind(total_amount)
        .bind(items.len() as i32)
        .bind(created_by_user_id)
        .bind(notes)
        .execute(&mut *tx)
        .await?;
        let run_id = run_result.last_insert_id() as i32;

        for (employee_id, input, amount) in items {
            sqlx::query(
                r#"INSERT INTO payroll_items
                (run_id, employee_id, employee_address, amount, memo, status)
                VALUES (?, ?, ?, ?, ?, 'pending')"#,
            )
            .bind(run_id)
            .bind(*employee_id)
            .bind(&input.employee_address)
            .bind(amount)
            .bind(input.memo.as_deref())
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(run_id)
    }

    pub async fn find_run_by_id(&self, id: i32) -> AppResult<Option<PayrollRun>> {
        let sql = format!("SELECT {RUN_COLS} FROM payroll_runs WHERE id = ?");
        Ok(sqlx::query_as::<_, PayrollRun>(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn list_runs(&self, limit: i32, offset: i32) -> AppResult<Vec<PayrollRun>> {
        let sql = format!(
            "SELECT {RUN_COLS} FROM payroll_runs ORDER BY created_at DESC LIMIT ? OFFSET ?"
        );
        Ok(sqlx::query_as::<_, PayrollRun>(&sql)
            .bind(limit)
            .bind(offset)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn list_items_by_run(&self, run_id: i32) -> AppResult<Vec<PayrollItem>> {
        let sql = format!(
            "SELECT {ITEM_COLS} FROM payroll_items WHERE run_id = ? ORDER BY id ASC"
        );
        Ok(sqlx::query_as::<_, PayrollItem>(&sql)
            .bind(run_id)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn find_item(&self, run_id: i32, item_id: i32) -> AppResult<Option<PayrollItem>> {
        let sql = format!(
            "SELECT {ITEM_COLS} FROM payroll_items WHERE run_id = ? AND id = ?"
        );
        Ok(sqlx::query_as::<_, PayrollItem>(&sql)
            .bind(run_id)
            .bind(item_id)
            .fetch_optional(&self.pool)
            .await?)
    }

    /// Status transitions: pending → executing → completed | partial | failed | canceled.
    /// We do not enforce the FSM in SQL; the service layer is the single
    /// caller and validates before invoking this.
    pub async fn update_run_status(
        &self,
        id: i32,
        status: &str,
        executed_by_user_id: Option<i32>,
        executed_at: Option<DateTime<Utc>>,
    ) -> AppResult<()> {
        sqlx::query(
            r#"UPDATE payroll_runs
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

    /// Mark an item submitted on-chain — bumps retry_count + last_attempt_at
    /// so a separately-running checker can detect stalled items.
    pub async fn mark_item_submitted(
        &self,
        item_id: i32,
        tx_hash: &str,
        transfer_id: Option<i32>,
    ) -> AppResult<()> {
        sqlx::query(
            r#"UPDATE payroll_items
               SET status = 'submitted', tx_hash = ?, transfer_id = COALESCE(?, transfer_id),
                   error_message = NULL, retry_count = retry_count + 1, last_attempt_at = CURRENT_TIMESTAMP
               WHERE id = ?"#,
        )
        .bind(tx_hash)
        .bind(transfer_id)
        .bind(item_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_item_failed(&self, item_id: i32, error_message: &str) -> AppResult<()> {
        sqlx::query(
            r#"UPDATE payroll_items
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

    /// Aggregate counts used by execute() to decide the final run status.
    pub async fn count_items_by_status(&self, run_id: i32) -> AppResult<ItemStatusCounts> {
        let row: (i64, i64, i64, i64) = sqlx::query_as(
            r#"SELECT
                 SUM(status = 'pending')   AS pending,
                 SUM(status = 'submitted') AS submitted,
                 SUM(status = 'failed')    AS failed,
                 COUNT(*)                  AS total
               FROM payroll_items WHERE run_id = ?"#,
        )
        .bind(run_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(ItemStatusCounts {
            pending: row.0,
            submitted: row.1,
            failed: row.2,
            total: row.3,
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ItemStatusCounts {
    pub pending: i64,
    pub submitted: i64,
    pub failed: i64,
    pub total: i64,
}
