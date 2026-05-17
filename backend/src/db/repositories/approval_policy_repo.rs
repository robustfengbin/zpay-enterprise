use rust_decimal::Decimal;
use sqlx::MySqlPool;

use crate::db::models_m1::ApprovalPolicy;
use crate::error::AppResult;

pub struct ApprovalPolicyRepository {
    pool: MySqlPool,
}

const COLS: &str =
    "id, scope, scope_id, chain, token, amount_threshold, sla_minutes, required_count, enabled, created_by, created_at, updated_at";

impl ApprovalPolicyRepository {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        scope: &str,
        scope_id: Option<i32>,
        chain: &str,
        token: &str,
        amount_threshold: Decimal,
        sla_minutes: i32,
        required_count: i32,
        enabled: bool,
        created_by: i32,
    ) -> AppResult<i32> {
        let result = sqlx::query(
            "INSERT INTO approval_policies
                (scope, scope_id, chain, token, amount_threshold, sla_minutes, required_count, enabled, created_by)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(scope)
        .bind(scope_id)
        .bind(chain)
        .bind(token)
        .bind(amount_threshold)
        .bind(sla_minutes)
        .bind(required_count)
        .bind(if enabled { 1i8 } else { 0i8 })
        .bind(created_by)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_id() as i32)
    }

    pub async fn find_by_id(&self, id: i32) -> AppResult<Option<ApprovalPolicy>> {
        let sql = format!("SELECT {COLS} FROM approval_policies WHERE id = ?");
        Ok(sqlx::query_as::<_, ApprovalPolicy>(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn list_all(&self) -> AppResult<Vec<ApprovalPolicy>> {
        // Order: most specific scopes first (user > wallet > global), then by amount desc.
        // Frontend Admin Policies page shows all; query keeps a stable order.
        let sql = format!(
            "SELECT {COLS} FROM approval_policies
             ORDER BY FIELD(scope, 'user', 'wallet', 'global'), amount_threshold DESC, id DESC"
        );
        Ok(sqlx::query_as::<_, ApprovalPolicy>(&sql)
            .fetch_all(&self.pool)
            .await?)
    }

    /// Find the policies that would match a given (chain, token, wallet_id, user_id)
    /// tuple — ordered most-specific-first.  Used by the approval-required check.
    pub async fn find_matching(
        &self,
        chain: &str,
        token: &str,
        wallet_id: i32,
        user_id: i32,
    ) -> AppResult<Vec<ApprovalPolicy>> {
        let sql = format!(
            "SELECT {COLS} FROM approval_policies
             WHERE enabled = 1
             AND chain = ?
             AND token = ?
             AND (
                  (scope = 'user'   AND scope_id = ?)
               OR (scope = 'wallet' AND scope_id = ?)
               OR (scope = 'global' AND scope_id IS NULL)
             )
             ORDER BY FIELD(scope, 'user', 'wallet', 'global'), amount_threshold ASC"
        );
        Ok(sqlx::query_as::<_, ApprovalPolicy>(&sql)
            .bind(chain)
            .bind(token)
            .bind(user_id)
            .bind(wallet_id)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn delete(&self, id: i32) -> AppResult<()> {
        sqlx::query("DELETE FROM approval_policies WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn set_enabled(&self, id: i32, enabled: bool) -> AppResult<()> {
        sqlx::query("UPDATE approval_policies SET enabled = ? WHERE id = ?")
            .bind(if enabled { 1i8 } else { 0i8 })
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// PATCH-style update of mutable fields.  scope / chain / token /
    /// created_by are immutable by design (see UpdateApprovalPolicyRequest).
    pub async fn update(
        &self,
        id: i32,
        amount_threshold: Option<Decimal>,
        sla_minutes: Option<i32>,
        required_count: Option<i32>,
        enabled: Option<bool>,
    ) -> AppResult<()> {
        let mut sets: Vec<&str> = Vec::new();
        if amount_threshold.is_some() {
            sets.push("amount_threshold = ?");
        }
        if sla_minutes.is_some() {
            sets.push("sla_minutes = ?");
        }
        if required_count.is_some() {
            sets.push("required_count = ?");
        }
        if enabled.is_some() {
            sets.push("enabled = ?");
        }
        if sets.is_empty() {
            return Ok(());
        }
        let sql = format!(
            "UPDATE approval_policies SET {} WHERE id = ?",
            sets.join(", ")
        );
        let mut q = sqlx::query(&sql);
        if let Some(v) = amount_threshold {
            q = q.bind(v);
        }
        if let Some(v) = sla_minutes {
            q = q.bind(v);
        }
        if let Some(v) = required_count {
            q = q.bind(v);
        }
        if let Some(v) = enabled {
            q = q.bind(if v { 1i8 } else { 0i8 });
        }
        q.bind(id).execute(&self.pool).await?;
        Ok(())
    }
}
