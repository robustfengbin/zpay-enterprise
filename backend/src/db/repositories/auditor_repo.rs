use sqlx::MySqlPool;

use crate::db::models_m1::{Auditor, AuditorWalletScope};
use crate::error::AppResult;

pub struct AuditorRepository {
    pool: MySqlPool,
}

const AUDITOR_COLS: &str =
    "id, email, password_hash, name, invited_by_user_id, active, last_login_at, created_at";

const SCOPE_COLS: &str = "id, auditor_id, wallet_id, granted_by_user_id, scope_start_ts, scope_end_ts, max_disclosure_count, current_count, created_at";

impl AuditorRepository {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        email: &str,
        password_hash: &str,
        name: &str,
        invited_by_user_id: i32,
    ) -> AppResult<i32> {
        let result = sqlx::query(
            "INSERT INTO auditors (email, password_hash, name, invited_by_user_id) VALUES (?, ?, ?, ?)",
        )
        .bind(email)
        .bind(password_hash)
        .bind(name)
        .bind(invited_by_user_id)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_id() as i32)
    }

    pub async fn find_by_email(&self, email: &str) -> AppResult<Option<Auditor>> {
        let sql = format!("SELECT {AUDITOR_COLS} FROM auditors WHERE email = ?");
        Ok(sqlx::query_as::<_, Auditor>(&sql)
            .bind(email)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn find_by_id(&self, id: i32) -> AppResult<Option<Auditor>> {
        let sql = format!("SELECT {AUDITOR_COLS} FROM auditors WHERE id = ?");
        Ok(sqlx::query_as::<_, Auditor>(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn list_all(&self) -> AppResult<Vec<Auditor>> {
        let sql = format!("SELECT {AUDITOR_COLS} FROM auditors ORDER BY id DESC");
        Ok(sqlx::query_as::<_, Auditor>(&sql)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn set_active(&self, id: i32, active: bool) -> AppResult<()> {
        sqlx::query("UPDATE auditors SET active = ? WHERE id = ?")
            .bind(active)
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn update_last_login(&self, id: i32) -> AppResult<()> {
        sqlx::query("UPDATE auditors SET last_login_at = CURRENT_TIMESTAMP WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // AuditorWalletScope — kept in same file as the Auditor itself; the
    // scope rows are meaningless without their owning auditor and they share
    // the auditor lifecycle for ON DELETE CASCADE.
    // -----------------------------------------------------------------------

    pub async fn create_scope(
        &self,
        auditor_id: i32,
        wallet_id: i32,
        granted_by_user_id: i32,
        scope_start_ts: chrono::DateTime<chrono::Utc>,
        scope_end_ts: chrono::DateTime<chrono::Utc>,
        max_disclosure_count: i32,
    ) -> AppResult<i32> {
        let result = sqlx::query(
            "INSERT INTO auditor_wallet_scopes
                (auditor_id, wallet_id, granted_by_user_id, scope_start_ts, scope_end_ts, max_disclosure_count)
             VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(auditor_id)
        .bind(wallet_id)
        .bind(granted_by_user_id)
        .bind(scope_start_ts)
        .bind(scope_end_ts)
        .bind(max_disclosure_count)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_id() as i32)
    }

    pub async fn list_scopes_for_auditor(
        &self,
        auditor_id: i32,
    ) -> AppResult<Vec<AuditorWalletScope>> {
        let sql = format!(
            "SELECT {SCOPE_COLS} FROM auditor_wallet_scopes WHERE auditor_id = ? ORDER BY id ASC"
        );
        Ok(sqlx::query_as::<_, AuditorWalletScope>(&sql)
            .bind(auditor_id)
            .fetch_all(&self.pool)
            .await?)
    }

    /// Returns the scope row if the auditor has an active grant for this wallet
    /// at the given moment; returns None when out of scope.  Caller uses this
    /// to authorise reads.
    pub async fn find_active_scope(
        &self,
        auditor_id: i32,
        wallet_id: i32,
        now: chrono::DateTime<chrono::Utc>,
    ) -> AppResult<Option<AuditorWalletScope>> {
        let sql = format!(
            "SELECT {SCOPE_COLS} FROM auditor_wallet_scopes
             WHERE auditor_id = ? AND wallet_id = ?
             AND scope_start_ts <= ? AND scope_end_ts >= ?
             LIMIT 1"
        );
        Ok(sqlx::query_as::<_, AuditorWalletScope>(&sql)
            .bind(auditor_id)
            .bind(wallet_id)
            .bind(now)
            .bind(now)
            .fetch_optional(&self.pool)
            .await?)
    }
}
