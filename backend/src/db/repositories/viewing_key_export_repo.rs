use chrono::{DateTime, Duration, Utc};
use sqlx::MySqlPool;

use crate::db::models_m1::ViewingKeyExport;
use crate::error::AppResult;

#[derive(Clone)]
pub struct ViewingKeyExportRepository {
    pool: MySqlPool,
}

const COLS: &str = "id, wallet_id, exported_by_user_id, key_type, encrypted_payload, payload_hash, download_token, downloaded_at, downloaded_by_ip, expires_at, created_at";

impl ViewingKeyExportRepository {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        wallet_id: i32,
        exported_by_user_id: i32,
        key_type: &str,
        encrypted_payload: &[u8],
        payload_hash: &str,
        download_token: &str,
    ) -> AppResult<(i32, DateTime<Utc>)> {
        // 24h TTL per F1.1 NFR-1 / FR-F1.1.11.
        let expires_at: DateTime<Utc> = Utc::now() + Duration::hours(24);

        let result = sqlx::query(
            "INSERT INTO viewing_key_exports
                (wallet_id, exported_by_user_id, key_type, encrypted_payload, payload_hash, download_token, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(wallet_id)
        .bind(exported_by_user_id)
        .bind(key_type)
        .bind(encrypted_payload)
        .bind(payload_hash)
        .bind(download_token)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok((result.last_insert_id() as i32, expires_at))
    }

    pub async fn find_by_id(&self, id: i32) -> AppResult<Option<ViewingKeyExport>> {
        let sql = format!("SELECT {COLS} FROM viewing_key_exports WHERE id = ?");
        Ok(sqlx::query_as::<_, ViewingKeyExport>(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn find_by_token(&self, token: &str) -> AppResult<Option<ViewingKeyExport>> {
        let sql = format!("SELECT {COLS} FROM viewing_key_exports WHERE download_token = ?");
        Ok(sqlx::query_as::<_, ViewingKeyExport>(&sql)
            .bind(token)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn list_by_wallet(&self, wallet_id: i32) -> AppResult<Vec<ViewingKeyExport>> {
        let sql = format!(
            "SELECT {COLS} FROM viewing_key_exports WHERE wallet_id = ? ORDER BY id DESC"
        );
        Ok(sqlx::query_as::<_, ViewingKeyExport>(&sql)
            .bind(wallet_id)
            .fetch_all(&self.pool)
            .await?)
    }

    /// Atomically mark the row downloaded and zero the encrypted payload.
    /// Returns Ok(true) if the row was claimed by this call, Ok(false) if
    /// it had already been downloaded.  This is the one-time-download enforcement.
    pub async fn claim_for_download(
        &self,
        id: i32,
        ip: &str,
    ) -> AppResult<bool> {
        let result = sqlx::query(
            "UPDATE viewing_key_exports
             SET downloaded_at = CURRENT_TIMESTAMP,
                 downloaded_by_ip = ?,
                 encrypted_payload = ''
             WHERE id = ?
             AND downloaded_at IS NULL
             AND expires_at > CURRENT_TIMESTAMP",
        )
        .bind(ip)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}
