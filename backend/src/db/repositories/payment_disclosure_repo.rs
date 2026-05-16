use chrono::{DateTime, Duration, Utc};
use sqlx::MySqlPool;

use crate::db::models_m1::PaymentDisclosure;
use crate::error::AppResult;

#[derive(Clone)]
pub struct PaymentDisclosureRepository {
    pool: MySqlPool,
}

const COLS: &str = "id, wallet_id, generated_by_user_id, granularity, scope_param, tx_count, disclosure_json, format, file_path, status, error_message, expires_at, created_at";

impl PaymentDisclosureRepository {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        wallet_id: i32,
        generated_by_user_id: i32,
        granularity: &str,
        scope_param: &serde_json::Value,
        format: &str,
    ) -> AppResult<i32> {
        // TTL 7 days per F1.1 NFR-4.
        let expires_at: DateTime<Utc> = Utc::now() + Duration::days(7);

        let result = sqlx::query(
            "INSERT INTO payment_disclosures
                (wallet_id, generated_by_user_id, granularity, scope_param, format, status, expires_at)
             VALUES (?, ?, ?, ?, ?, 'generating', ?)",
        )
        .bind(wallet_id)
        .bind(generated_by_user_id)
        .bind(granularity)
        .bind(scope_param)
        .bind(format)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_id() as i32)
    }

    pub async fn find_by_id(&self, id: i32) -> AppResult<Option<PaymentDisclosure>> {
        let sql = format!("SELECT {COLS} FROM payment_disclosures WHERE id = ?");
        Ok(sqlx::query_as::<_, PaymentDisclosure>(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn list_by_wallet(&self, wallet_id: i32) -> AppResult<Vec<PaymentDisclosure>> {
        let sql = format!(
            "SELECT {COLS} FROM payment_disclosures WHERE wallet_id = ? ORDER BY id DESC"
        );
        Ok(sqlx::query_as::<_, PaymentDisclosure>(&sql)
            .bind(wallet_id)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn mark_ready(
        &self,
        id: i32,
        disclosure_json: &serde_json::Value,
        tx_count: i32,
        file_path: Option<&str>,
    ) -> AppResult<()> {
        sqlx::query(
            "UPDATE payment_disclosures
             SET status = 'ready', disclosure_json = ?, tx_count = ?, file_path = ?
             WHERE id = ? AND status = 'generating'",
        )
        .bind(disclosure_json)
        .bind(tx_count)
        .bind(file_path)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn mark_failed(&self, id: i32, error: &str) -> AppResult<()> {
        sqlx::query(
            "UPDATE payment_disclosures
             SET status = 'failed', error_message = ?
             WHERE id = ? AND status = 'generating'",
        )
        .bind(error)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}
