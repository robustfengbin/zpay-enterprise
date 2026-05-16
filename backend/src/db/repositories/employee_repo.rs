use crate::db::models_m1::Employee;
use crate::error::AppResult;
use sqlx::MySqlPool;

pub struct EmployeeRepository {
    pool: MySqlPool,
}

const COLS: &str = "id, employee_code, name, wallet_address, chain, tags, active, created_at, updated_at";

impl EmployeeRepository {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }

    pub async fn create(
        &self,
        employee_code: &str,
        name: &str,
        wallet_address: &str,
        chain: &str,
        tags: Option<&serde_json::Value>,
    ) -> AppResult<i32> {
        let result = sqlx::query(
            "INSERT INTO employees (employee_code, name, wallet_address, chain, tags) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(employee_code)
        .bind(name)
        .bind(wallet_address)
        .bind(chain)
        .bind(tags)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_id() as i32)
    }

    pub async fn find_by_id(&self, id: i32) -> AppResult<Option<Employee>> {
        let sql = format!("SELECT {COLS} FROM employees WHERE id = ?");
        Ok(sqlx::query_as::<_, Employee>(&sql)
            .bind(id)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn find_by_code(&self, code: &str) -> AppResult<Option<Employee>> {
        let sql = format!("SELECT {COLS} FROM employees WHERE employee_code = ?");
        Ok(sqlx::query_as::<_, Employee>(&sql)
            .bind(code)
            .fetch_optional(&self.pool)
            .await?)
    }

    pub async fn list(&self, active_only: bool) -> AppResult<Vec<Employee>> {
        let sql = if active_only {
            format!("SELECT {COLS} FROM employees WHERE active = TRUE ORDER BY id DESC")
        } else {
            format!("SELECT {COLS} FROM employees ORDER BY id DESC")
        };
        Ok(sqlx::query_as::<_, Employee>(&sql)
            .fetch_all(&self.pool)
            .await?)
    }

    pub async fn update(
        &self,
        id: i32,
        name: Option<&str>,
        wallet_address: Option<&str>,
        tags: Option<&serde_json::Value>,
        active: Option<bool>,
    ) -> AppResult<()> {
        // Build dynamic SET clause — only update provided fields. Keeps the
        // contract narrow: partial PATCH semantics, no surprise overwrites.
        let mut sets: Vec<&str> = Vec::new();
        if name.is_some() {
            sets.push("name = ?");
        }
        if wallet_address.is_some() {
            sets.push("wallet_address = ?");
        }
        if tags.is_some() {
            sets.push("tags = ?");
        }
        if active.is_some() {
            sets.push("active = ?");
        }
        if sets.is_empty() {
            return Ok(());
        }

        let sql = format!("UPDATE employees SET {} WHERE id = ?", sets.join(", "));
        let mut q = sqlx::query(&sql);
        if let Some(v) = name {
            q = q.bind(v);
        }
        if let Some(v) = wallet_address {
            q = q.bind(v);
        }
        if let Some(v) = tags {
            q = q.bind(v);
        }
        if let Some(v) = active {
            q = q.bind(v);
        }
        q.bind(id).execute(&self.pool).await?;
        Ok(())
    }

    /// Soft delete: set active=false. Hard delete is intentionally not
    /// exposed — historical payroll_items reference employee_id.
    pub async fn soft_delete(&self, id: i32) -> AppResult<()> {
        sqlx::query("UPDATE employees SET active = FALSE WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
