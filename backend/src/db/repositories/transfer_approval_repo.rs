use sqlx::MySqlPool;

use crate::db::models_m1::TransferApproval;
use crate::error::AppResult;

#[derive(Clone)]
pub struct TransferApprovalRepository {
    pool: MySqlPool,
}

const COLS: &str = "id, transfer_id, approver_user_id, decision, reason, policy_snapshot, idempotency_key, created_at";

impl TransferApprovalRepository {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        transfer_id: i32,
        approver_user_id: i32,
        decision: &str,
        reason: Option<&str>,
        policy_snapshot: Option<&serde_json::Value>,
        idempotency_key: Option<&str>,
    ) -> AppResult<i32> {
        let result = sqlx::query(
            "INSERT INTO transfer_approvals
                (transfer_id, approver_user_id, decision, reason, policy_snapshot, idempotency_key)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(transfer_id)
        .bind(approver_user_id)
        .bind(decision)
        .bind(reason)
        .bind(policy_snapshot)
        .bind(idempotency_key)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_id() as i32)
    }

    pub async fn list_by_transfer(&self, transfer_id: i32) -> AppResult<Vec<TransferApproval>> {
        let sql = format!(
            "SELECT {COLS} FROM transfer_approvals WHERE transfer_id = ? ORDER BY id ASC"
        );
        Ok(sqlx::query_as::<_, TransferApproval>(&sql)
            .bind(transfer_id)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn find_by_idempotency_key(
        &self,
        key: &str,
    ) -> AppResult<Option<TransferApproval>> {
        let sql = format!("SELECT {COLS} FROM transfer_approvals WHERE idempotency_key = ?");
        Ok(sqlx::query_as::<_, TransferApproval>(&sql)
            .bind(key)
            .fetch_optional(&self.pool)
            .await?)
    }
}
