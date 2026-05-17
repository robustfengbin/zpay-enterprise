use crate::db::models::Transfer;
use crate::error::AppResult;
use rust_decimal::Decimal;
use sqlx::MySqlPool;

pub struct TransferRepository {
    pool: MySqlPool,
}

impl TransferRepository {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        wallet_id: i32,
        chain: &str,
        from_address: &str,
        to_address: &str,
        token: &str,
        amount: Decimal,
        gas_price: Option<Decimal>,
        gas_limit: Option<i64>,
        initiated_by: i32,
    ) -> AppResult<i32> {
        let result = sqlx::query(
            r#"INSERT INTO transfers
            (wallet_id, chain, from_address, to_address, token, amount, gas_price, gas_limit, status, initiated_by)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?)"#
        )
        .bind(wallet_id)
        .bind(chain)
        .bind(from_address)
        .bind(to_address)
        .bind(token)
        .bind(amount)
        .bind(gas_price)
        .bind(gas_limit)
        .bind(initiated_by)
        .execute(&self.pool)
        .await?;

        Ok(result.last_insert_id() as i32)
    }

    /// M1 F2.1 — create a transfer that is gated by approval.  The caller
    /// has already matched a policy and computed the SLA expiry; this
    /// repo just persists the row with the right status + bookkeeping
    /// columns set in a single insert so the row is never observed in
    /// an inconsistent state.
    #[allow(clippy::too_many_arguments)]
    pub async fn create_awaiting_approval(
        &self,
        wallet_id: i32,
        chain: &str,
        from_address: &str,
        to_address: &str,
        token: &str,
        amount: Decimal,
        gas_price: Option<Decimal>,
        gas_limit: Option<i64>,
        initiated_by: i32,
        expiry_at: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<i32> {
        let result = sqlx::query(
            r#"INSERT INTO transfers
            (wallet_id, chain, from_address, to_address, token, amount, gas_price, gas_limit, status, initiated_by, expiry_at, approval_required)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, 'awaiting_approval', ?, ?, 1)"#,
        )
        .bind(wallet_id)
        .bind(chain)
        .bind(from_address)
        .bind(to_address)
        .bind(token)
        .bind(amount)
        .bind(gas_price)
        .bind(gas_limit)
        .bind(initiated_by)
        .bind(expiry_at)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_id() as i32)
    }

    pub async fn find_by_id(&self, id: i32) -> AppResult<Option<Transfer>> {
        let transfer = sqlx::query_as::<_, Transfer>(
            "SELECT * FROM transfers WHERE id = ?"
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(transfer)
    }

    pub async fn update_status(
        &self,
        id: i32,
        status: &str,
        tx_hash: Option<&str>,
        error_message: Option<&str>,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE transfers SET status = ?, tx_hash = ?, error_message = ? WHERE id = ?"
        )
        .bind(status)
        .bind(tx_hash)
        .bind(error_message)
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn update_confirmed(
        &self,
        id: i32,
        block_number: i64,
        gas_used: i64,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE transfers SET status = 'confirmed', block_number = ?, gas_used = ? WHERE id = ?"
        )
        .bind(block_number)
        .bind(gas_used)
        .bind(id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn list_by_wallet(&self, wallet_id: i32, limit: i32, offset: i32) -> AppResult<Vec<Transfer>> {
        let transfers = sqlx::query_as::<_, Transfer>(
            "SELECT * FROM transfers WHERE wallet_id = ? ORDER BY created_at DESC LIMIT ? OFFSET ?"
        )
        .bind(wallet_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        Ok(transfers)
    }

    /// M1 F1.1 — list wallet transfers within an auditor scope's time window.
    /// `start` is inclusive, `end` exclusive so a scope ending at 00:00 of a
    /// given day does not bleed into that day's traffic.
    pub async fn list_by_wallet_in_window(
        &self,
        wallet_id: i32,
        start: chrono::DateTime<chrono::Utc>,
        end: chrono::DateTime<chrono::Utc>,
        limit: i32,
        offset: i32,
    ) -> AppResult<Vec<Transfer>> {
        let transfers = sqlx::query_as::<_, Transfer>(
            r#"SELECT * FROM transfers
               WHERE wallet_id = ?
                 AND created_at >= ?
                 AND created_at < ?
               ORDER BY created_at DESC
               LIMIT ? OFFSET ?"#,
        )
        .bind(wallet_id)
        .bind(start)
        .bind(end)
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        Ok(transfers)
    }

    pub async fn list_all(&self, limit: i32, offset: i32) -> AppResult<Vec<Transfer>> {
        let transfers = sqlx::query_as::<_, Transfer>(
            "SELECT * FROM transfers ORDER BY created_at DESC LIMIT ? OFFSET ?"
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        Ok(transfers)
    }

    pub async fn list_pending(&self) -> AppResult<Vec<Transfer>> {
        let transfers = sqlx::query_as::<_, Transfer>(
            "SELECT * FROM transfers WHERE status = 'submitted' ORDER BY created_at"
        )
        .fetch_all(&self.pool)
        .await?;

        Ok(transfers)
    }

    pub async fn count_all(&self) -> AppResult<i64> {
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM transfers")
            .fetch_one(&self.pool)
            .await?;

        Ok(count.0)
    }

    pub async fn count_by_wallet(&self, wallet_id: i32) -> AppResult<i64> {
        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM transfers WHERE wallet_id = ?")
            .bind(wallet_id)
            .fetch_one(&self.pool)
            .await?;

        Ok(count.0)
    }

    /// M1 F2.1 — flip every `awaiting_approval` transfer whose SLA deadline has
    /// passed into the terminal `expired` state.  Returns the row count so the
    /// caller can decide whether to log.  Driven by the SLA worker.
    pub async fn expire_overdue_awaiting_approval(&self) -> AppResult<u64> {
        let result = sqlx::query(
            r#"UPDATE transfers
               SET status = 'expired'
               WHERE status = 'awaiting_approval'
                 AND expiry_at IS NOT NULL
                 AND expiry_at < NOW()"#,
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}
