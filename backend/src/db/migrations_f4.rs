//! F4 2026-07 migrations for the batch privacy transfer & Ironwood migration
//! module (PRD-F4, docs/2026-07-24/):
//!   - F4.1 Orchard → Ironwood migration engine (2 tables)
//!   - F4.2 generic batch privacy transfer (2 tables, schema finalized now,
//!     execution layer lands after F4.0 merges)
//!
//! All migrations are additive and backward-compatible:
//!   - New tables use CREATE TABLE IF NOT EXISTS
//!   - Existing API/route behavior is not changed
//!
//! Per CLAUDE.md C-2: schema changes execute at service startup via
//! `run_f4_migrations`, called from `db::run_migrations`. No .sql files.

use sqlx::MySqlPool;

use crate::error::AppResult;

pub async fn run_f4_migrations(pool: &MySqlPool) -> AppResult<()> {
    tracing::info!("[F4 migrations] starting...");

    create_f41_migration_tables(pool).await?;
    create_f42_batch_transfer_tables(pool).await?;

    tracing::info!("[F4 migrations] done");
    Ok(())
}

// ---------------------------------------------------------------------------
// F4.1 — Orchard → Ironwood migration runs
// ---------------------------------------------------------------------------

async fn create_f41_migration_tables(pool: &MySqlPool) -> AppResult<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS migration_runs (
            id INT PRIMARY KEY AUTO_INCREMENT,
            source_wallet_id INT NOT NULL,
            mode VARCHAR(16) NOT NULL,
            batch_count INT NOT NULL,
            window_hours INT NOT NULL DEFAULT 0,
            total_amount DECIMAL(36, 18) NOT NULL,
            item_count INT NOT NULL,
            status VARCHAR(32) NOT NULL DEFAULT 'pending',
            created_by_user_id INT NOT NULL,
            approved_by_user_id INT NULL,
            reject_reason VARCHAR(255) NULL,
            executed_by_user_id INT NULL,
            executed_at TIMESTAMP NULL,
            notes VARCHAR(500) NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
            FOREIGN KEY (source_wallet_id) REFERENCES wallets(id),
            INDEX idx_migration_runs_wallet (source_wallet_id),
            INDEX idx_migration_runs_status (status)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS migration_items (
            id INT PRIMARY KEY AUTO_INCREMENT,
            run_id INT NOT NULL,
            seq INT NOT NULL,
            amount DECIMAL(36, 18) NOT NULL,
            scheduled_at TIMESTAMP NULL,
            status VARCHAR(32) NOT NULL DEFAULT 'pending',
            tx_hash VARCHAR(128) NULL,
            error_message TEXT NULL,
            retry_count INT NOT NULL DEFAULT 0,
            last_attempt_at TIMESTAMP NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
            FOREIGN KEY (run_id) REFERENCES migration_runs(id) ON DELETE CASCADE,
            INDEX idx_migration_items_run (run_id),
            INDEX idx_migration_items_due (status, scheduled_at)
        )
        "#,
    )
    .execute(pool)
    .await?;

    tracing::info!("[F4 migrations] F4.1 migration_runs / migration_items ready");
    Ok(())
}

// ---------------------------------------------------------------------------
// F4.2 — generic batch privacy transfer (schema only for now; the execution
// layer reuses the F4.1 executor after F4.0 merges — PRD-F4 D2 option A)
// ---------------------------------------------------------------------------

async fn create_f42_batch_transfer_tables(pool: &MySqlPool) -> AppResult<()> {
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS batch_transfer_runs (
            id INT PRIMARY KEY AUTO_INCREMENT,
            title VARCHAR(120) NOT NULL,
            source_wallet_id INT NOT NULL,
            privacy_mode VARCHAR(16) NOT NULL DEFAULT 'off',
            batch_count INT NOT NULL DEFAULT 1,
            window_hours INT NOT NULL DEFAULT 0,
            total_amount DECIMAL(36, 18) NOT NULL,
            item_count INT NOT NULL,
            status VARCHAR(32) NOT NULL DEFAULT 'pending',
            created_by_user_id INT NOT NULL,
            approved_by_user_id INT NULL,
            reject_reason VARCHAR(255) NULL,
            executed_by_user_id INT NULL,
            executed_at TIMESTAMP NULL,
            notes VARCHAR(500) NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
            FOREIGN KEY (source_wallet_id) REFERENCES wallets(id),
            INDEX idx_batch_transfer_runs_wallet (source_wallet_id),
            INDEX idx_batch_transfer_runs_status (status)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS batch_transfer_items (
            id INT PRIMARY KEY AUTO_INCREMENT,
            run_id INT NOT NULL,
            seq INT NOT NULL,
            recipient_address VARCHAR(512) NOT NULL,
            amount DECIMAL(36, 18) NOT NULL,
            memo VARCHAR(512) NULL,
            scheduled_at TIMESTAMP NULL,
            status VARCHAR(32) NOT NULL DEFAULT 'pending',
            tx_hash VARCHAR(128) NULL,
            error_message TEXT NULL,
            retry_count INT NOT NULL DEFAULT 0,
            last_attempt_at TIMESTAMP NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
            FOREIGN KEY (run_id) REFERENCES batch_transfer_runs(id) ON DELETE CASCADE,
            INDEX idx_batch_transfer_items_run (run_id),
            INDEX idx_batch_transfer_items_due (status, scheduled_at)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Tables created before the F4.2 execution layer landed lack
    // reject_reason (schema was finalized first, PRD F4.2.1); add it in
    // place — same guarded-ALTER pattern as db::run_migrations.
    let reject_reason_exists: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT COLUMN_NAME FROM INFORMATION_SCHEMA.COLUMNS
        WHERE TABLE_SCHEMA = DATABASE()
          AND TABLE_NAME = 'batch_transfer_runs'
          AND COLUMN_NAME = 'reject_reason'
        "#,
    )
    .fetch_optional(pool)
    .await?;
    if reject_reason_exists.is_none() {
        sqlx::query(
            "ALTER TABLE batch_transfer_runs ADD COLUMN reject_reason VARCHAR(255) NULL AFTER approved_by_user_id",
        )
        .execute(pool)
        .await?;
        tracing::info!("[F4 migrations] added batch_transfer_runs.reject_reason");
    }

    tracing::info!("[F4 migrations] F4.2 batch_transfer_runs / batch_transfer_items ready");
    Ok(())
}
