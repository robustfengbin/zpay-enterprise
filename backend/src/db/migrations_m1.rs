//! M1 2026-06 migrations for three P0 features:
//!   - F1.1 Viewing Key audit layer (4 tables)
//!   - F2.1 Maker/Checker dual-sign (2 tables + 5 columns on transfers)
//!   - F3.1 batch Payroll Run (3 tables + 1 column on transfers)
//!
//! All migrations are additive and backward-compatible:
//!   - New tables use CREATE TABLE IF NOT EXISTS
//!   - New columns on existing tables are nullable or have defaults
//!   - Existing API/route behavior is not changed
//!
//! Per CLAUDE.md C-2: schema changes execute at service startup via
//! `run_m1_migrations`, called from `db::run_migrations`. No .sql files.

use sqlx::MySqlPool;

use crate::error::AppResult;

pub async fn run_m1_migrations(pool: &MySqlPool) -> AppResult<()> {
    tracing::info!("[M1 migrations] starting...");

    create_f11_viewing_key_audit_tables(pool).await?;
    create_f21_maker_checker_tables(pool).await?;
    create_f31_payroll_tables(pool).await?;
    extend_transfers_for_m1(pool).await?;
    seed_default_approval_policies(pool).await?;

    tracing::info!("[M1 migrations] done");
    Ok(())
}

// ---------------------------------------------------------------------------
// F1.1 — Viewing Key audit layer
// ---------------------------------------------------------------------------

async fn create_f11_viewing_key_audit_tables(pool: &MySqlPool) -> AppResult<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS auditors (
            id INT PRIMARY KEY AUTO_INCREMENT,
            email VARCHAR(255) NOT NULL UNIQUE,
            password_hash VARCHAR(255) NOT NULL,
            name VARCHAR(255) NOT NULL,
            invited_by_user_id INT NOT NULL,
            active BOOLEAN NOT NULL DEFAULT TRUE,
            last_login_at TIMESTAMP NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (invited_by_user_id) REFERENCES users(id),
            INDEX idx_auditors_email (email)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS auditor_wallet_scopes (
            id INT PRIMARY KEY AUTO_INCREMENT,
            auditor_id INT NOT NULL,
            wallet_id INT NOT NULL,
            granted_by_user_id INT NOT NULL,
            scope_start_ts TIMESTAMP NOT NULL,
            scope_end_ts TIMESTAMP NOT NULL,
            max_disclosure_count INT NOT NULL DEFAULT 10,
            current_count INT NOT NULL DEFAULT 0,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (auditor_id) REFERENCES auditors(id) ON DELETE CASCADE,
            FOREIGN KEY (wallet_id) REFERENCES wallets(id) ON DELETE CASCADE,
            UNIQUE KEY uniq_auditor_wallet_scope (auditor_id, wallet_id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS viewing_key_exports (
            id INT PRIMARY KEY AUTO_INCREMENT,
            wallet_id INT NOT NULL,
            exported_by_user_id INT NOT NULL,
            key_type VARCHAR(16) NOT NULL,
            encrypted_payload BLOB NOT NULL,
            payload_hash VARCHAR(64) NOT NULL,
            download_token VARCHAR(64) NOT NULL UNIQUE,
            downloaded_at TIMESTAMP NULL,
            downloaded_by_ip VARCHAR(64) NULL,
            expires_at TIMESTAMP NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (wallet_id) REFERENCES wallets(id),
            FOREIGN KEY (exported_by_user_id) REFERENCES users(id),
            INDEX idx_vke_wallet (wallet_id),
            INDEX idx_vke_token (download_token)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS payment_disclosures (
            id INT PRIMARY KEY AUTO_INCREMENT,
            wallet_id INT NOT NULL,
            generated_by_user_id INT NOT NULL,
            granularity VARCHAR(16) NOT NULL,
            scope_param JSON NOT NULL,
            tx_count INT NOT NULL DEFAULT 0,
            disclosure_json JSON NULL,
            format VARCHAR(16) NOT NULL,
            file_path VARCHAR(512) NULL,
            status VARCHAR(16) NOT NULL DEFAULT 'generating',
            error_message TEXT NULL,
            expires_at TIMESTAMP NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (wallet_id) REFERENCES wallets(id),
            FOREIGN KEY (generated_by_user_id) REFERENCES users(id),
            INDEX idx_pd_wallet_status (wallet_id, status)
        )
        "#,
    )
    .execute(pool)
    .await?;

    tracing::info!("[M1 migrations] F1.1 viewing-key-audit tables ready");
    Ok(())
}

// ---------------------------------------------------------------------------
// F2.1 — Maker / Checker dual-sign (see PRD-F2.1 §4)
// ---------------------------------------------------------------------------

async fn create_f21_maker_checker_tables(pool: &MySqlPool) -> AppResult<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS transfer_approvals (
            id INT PRIMARY KEY AUTO_INCREMENT,
            transfer_id INT NOT NULL,
            approver_user_id INT NOT NULL,
            decision VARCHAR(16) NOT NULL,
            reason TEXT NULL,
            policy_snapshot JSON NULL,
            idempotency_key VARCHAR(128) NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (transfer_id) REFERENCES transfers(id) ON DELETE CASCADE,
            FOREIGN KEY (approver_user_id) REFERENCES users(id),
            UNIQUE KEY uq_approval_idem (idempotency_key),
            UNIQUE KEY uq_one_decision_per_approver (transfer_id, approver_user_id),
            INDEX idx_ta_transfer (transfer_id),
            INDEX idx_ta_approver (approver_user_id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS approval_policies (
            id INT PRIMARY KEY AUTO_INCREMENT,
            scope VARCHAR(16) NOT NULL,
            scope_id INT NULL,
            chain VARCHAR(32) NOT NULL,
            token VARCHAR(32) NOT NULL,
            amount_threshold DECIMAL(36, 18) NOT NULL,
            sla_minutes INT NOT NULL DEFAULT 1440,
            required_count INT NOT NULL DEFAULT 1,
            enabled TINYINT(1) NOT NULL DEFAULT 1,
            created_by INT NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
            FOREIGN KEY (created_by) REFERENCES users(id),
            UNIQUE KEY uq_policy_scope_match (scope, scope_id, chain, token),
            INDEX idx_ap_scope_lookup (scope, chain, token, enabled)
        )
        "#,
    )
    .execute(pool)
    .await?;

    tracing::info!("[M1 migrations] F2.1 maker-checker tables ready");
    Ok(())
}

// ---------------------------------------------------------------------------
// F3.1 — batch Payroll Run
// ---------------------------------------------------------------------------

async fn create_f31_payroll_tables(pool: &MySqlPool) -> AppResult<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS employees (
            id INT PRIMARY KEY AUTO_INCREMENT,
            employee_code VARCHAR(64) NOT NULL UNIQUE,
            name VARCHAR(255) NOT NULL,
            wallet_address VARCHAR(512) NOT NULL,
            chain VARCHAR(32) NOT NULL DEFAULT 'zcash',
            tags JSON NULL,
            active BOOLEAN NOT NULL DEFAULT TRUE,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
            INDEX idx_emp_active (active),
            INDEX idx_emp_address (wallet_address)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS payroll_runs (
            id INT PRIMARY KEY AUTO_INCREMENT,
            pay_period VARCHAR(32) NOT NULL,
            source_wallet_id INT NOT NULL,
            total_amount DECIMAL(36, 18) NOT NULL,
            item_count INT NOT NULL,
            status VARCHAR(24) NOT NULL DEFAULT 'pending',
            created_by_user_id INT NOT NULL,
            executed_by_user_id INT NULL,
            executed_at TIMESTAMP NULL,
            notes TEXT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
            FOREIGN KEY (source_wallet_id) REFERENCES wallets(id),
            FOREIGN KEY (created_by_user_id) REFERENCES users(id),
            FOREIGN KEY (executed_by_user_id) REFERENCES users(id),
            INDEX idx_pr_status (status),
            INDEX idx_pr_period (pay_period)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS payroll_items (
            id INT PRIMARY KEY AUTO_INCREMENT,
            run_id INT NOT NULL,
            employee_id INT NULL,
            employee_address VARCHAR(512) NOT NULL,
            amount DECIMAL(36, 18) NOT NULL,
            memo TEXT NULL,
            status VARCHAR(24) NOT NULL DEFAULT 'pending',
            tx_hash VARCHAR(128) NULL,
            block_number BIGINT NULL,
            transfer_id INT NULL,
            error_message TEXT NULL,
            retry_count INT NOT NULL DEFAULT 0,
            last_attempt_at TIMESTAMP NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
            FOREIGN KEY (run_id) REFERENCES payroll_runs(id) ON DELETE CASCADE,
            FOREIGN KEY (employee_id) REFERENCES employees(id),
            FOREIGN KEY (transfer_id) REFERENCES transfers(id),
            INDEX idx_pi_run_status (run_id, status),
            INDEX idx_pi_tx_hash (tx_hash)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Forward-only schema upgrade (2026-05-17): widen DECIMAL precision
    // from (28, 8) → (36, 18) so multi-chain payroll can hold any ERC20
    // / EVM-native amount without truncation (Gemini GitHub PR review).
    // MODIFY COLUMN is metadata-only when the new precision is a superset
    // of the old (which it is here) — no data loss, no row rewrite.
    // sqlx returns Err on already-modified columns? No — MODIFY is
    // idempotent: re-applying the same definition is a no-op.
    sqlx::query(
        r#"ALTER TABLE payroll_runs MODIFY COLUMN total_amount DECIMAL(36, 18) NOT NULL"#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"ALTER TABLE payroll_items MODIFY COLUMN amount DECIMAL(36, 18) NOT NULL"#,
    )
    .execute(pool)
    .await?;

    tracing::info!("[M1 migrations] F3.1 payroll tables ready");
    Ok(())
}

// ---------------------------------------------------------------------------
// Extend `transfers` table — additive columns for F2.1 + F3.1
// ---------------------------------------------------------------------------

async fn extend_transfers_for_m1(pool: &MySqlPool) -> AppResult<()> {
    // F2.1 columns
    add_column_if_missing(
        pool,
        "transfers",
        "expiry_at",
        "ADD COLUMN expiry_at TIMESTAMP NULL",
    )
    .await?;
    add_column_if_missing(
        pool,
        "transfers",
        "approval_required",
        "ADD COLUMN approval_required TINYINT(1) NOT NULL DEFAULT 0",
    )
    .await?;
    add_column_if_missing(
        pool,
        "transfers",
        "approved_at",
        "ADD COLUMN approved_at TIMESTAMP NULL",
    )
    .await?;
    add_column_if_missing(
        pool,
        "transfers",
        "approved_by",
        "ADD COLUMN approved_by INT NULL",
    )
    .await?;
    add_column_if_missing(
        pool,
        "transfers",
        "rejection_reason",
        "ADD COLUMN rejection_reason TEXT NULL",
    )
    .await?;

    // F3.1 column
    add_column_if_missing(
        pool,
        "transfers",
        "payroll_item_id",
        "ADD COLUMN payroll_item_id INT NULL",
    )
    .await?;

    tracing::info!("[M1 migrations] transfers column extensions applied");
    Ok(())
}

/// Helper: check `INFORMATION_SCHEMA.COLUMNS` then ALTER. Backward compatible
/// with MySQL versions that don't support `ADD COLUMN IF NOT EXISTS`.
async fn add_column_if_missing(
    pool: &MySqlPool,
    table: &str,
    column: &str,
    alter_clause: &str,
) -> AppResult<()> {
    let exists: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT COLUMN_NAME FROM INFORMATION_SCHEMA.COLUMNS
        WHERE TABLE_SCHEMA = DATABASE()
        AND TABLE_NAME = ?
        AND COLUMN_NAME = ?
        "#,
    )
    .bind(table)
    .bind(column)
    .fetch_optional(pool)
    .await?;

    if exists.is_none() {
        let sql = format!("ALTER TABLE {} {}", table, alter_clause);
        sqlx::query(&sql).execute(pool).await?;
        tracing::info!("[M1 migrations] added column {}.{}", table, column);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Seed default approval policies (F2.1 §4.5)
// ---------------------------------------------------------------------------

async fn seed_default_approval_policies(pool: &MySqlPool) -> AppResult<()> {
    // Only seed once: if any policy exists, do nothing.
    let existing: Option<(i64,)> = sqlx::query_as("SELECT COUNT(*) FROM approval_policies")
        .fetch_optional(pool)
        .await?;

    if let Some((count,)) = existing {
        if count > 0 {
            tracing::debug!(
                "[M1 migrations] approval_policies already seeded ({} rows), skipping",
                count
            );
            return Ok(());
        }
    }

    // High default thresholds so existing automated clients are not broken
    // by a sudden mandatory approval gate (F2.1 NFR-2 backward-compat).
    let seeds: &[(&str, &str, &str)] = &[
        ("ethereum", "USDT", "5000"),
        ("ethereum", "USDC", "5000"),
        ("ethereum", "DAI", "5000"),
        ("ethereum", "WETH", "5"),
        ("ethereum", "ETH", "5"),
        ("zcash", "ZEC", "1000"),
    ];

    // Resolve a creator user_id — use the first Admin user we find.
    // Migration runs after `users` table exists; admin is seeded by auth_service
    // on first boot.  If no admin yet (true fresh install), defer seeding —
    // a later boot will seed once admin exists.
    let admin: Option<(i32,)> =
        sqlx::query_as("SELECT id FROM users WHERE role = 'admin' ORDER BY id ASC LIMIT 1")
            .fetch_optional(pool)
            .await?;

    let Some((admin_id,)) = admin else {
        tracing::info!(
            "[M1 migrations] no admin user yet, deferring approval_policies seed to next boot"
        );
        return Ok(());
    };

    for (chain, token, threshold) in seeds {
        sqlx::query(
            r#"
            INSERT INTO approval_policies
                (scope, scope_id, chain, token, amount_threshold, sla_minutes, required_count, enabled, created_by)
            VALUES ('global', NULL, ?, ?, ?, 1440, 1, 1, ?)
            "#,
        )
        .bind(chain)
        .bind(token)
        .bind(threshold)
        .bind(admin_id)
        .execute(pool)
        .await?;
    }

    tracing::info!(
        "[M1 migrations] seeded {} default approval policies",
        seeds.len()
    );
    Ok(())
}
