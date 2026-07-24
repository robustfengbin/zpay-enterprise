pub mod migrations_f4;
pub mod migrations_m1;
pub mod models;
pub mod models_f4;
pub mod models_m1;
pub mod repositories;

use crate::config::DatabaseConfig;
use crate::error::AppResult;
use sqlx::mysql::MySqlPoolOptions;
use sqlx::MySqlPool;

pub async fn create_pool(config: &DatabaseConfig) -> AppResult<MySqlPool> {
    use std::time::Duration;

    let url = config.url();
    tracing::info!("Connecting to database at {}:{}/{}", config.host, config.port, config.name);

    let pool = MySqlPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(30))
        .idle_timeout(Duration::from_secs(600))
        .max_lifetime(Duration::from_secs(1800))
        .connect(&url)
        .await
        .map_err(|e| crate::error::AppError::DatabaseError(format!("Failed to connect to database: {}", e)))?;

    tracing::info!("Database connection pool created successfully (max: {}, min: 2)", config.max_connections);
    Ok(pool)
}

/// Log current database pool statistics
pub fn log_pool_stats(pool: &MySqlPool) {
    let size = pool.size();
    let idle = pool.num_idle();
    let active = size - idle as u32;

    if active > 15 {
        tracing::warn!(
            "[DB Pool] ⚠️ High usage: active={}, idle={}, total={}/20",
            active, idle, size
        );
    } else {
        tracing::debug!(
            "[DB Pool] Stats: active={}, idle={}, total={}/20",
            active, idle, size
        );
    }
}

pub async fn run_migrations(pool: &MySqlPool) -> AppResult<()> {
    // Create tables if they don't exist
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id INT PRIMARY KEY AUTO_INCREMENT,
            username VARCHAR(50) UNIQUE NOT NULL,
            password_hash VARCHAR(255) NOT NULL,
            role VARCHAR(20) DEFAULT 'operator',
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS wallets (
            id INT PRIMARY KEY AUTO_INCREMENT,
            name VARCHAR(100) NOT NULL,
            address VARCHAR(42) NOT NULL,
            encrypted_private_key TEXT NOT NULL,
            chain VARCHAR(20) DEFAULT 'ethereum',
            is_active BOOLEAN DEFAULT FALSE,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            UNIQUE KEY unique_address_chain (address, chain)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS transfers (
            id INT PRIMARY KEY AUTO_INCREMENT,
            wallet_id INT NOT NULL,
            chain VARCHAR(20) NOT NULL,
            from_address VARCHAR(42) NOT NULL,
            to_address VARCHAR(42) NOT NULL,
            token VARCHAR(20) NOT NULL,
            amount DECIMAL(36, 18) NOT NULL,
            gas_price DECIMAL(20, 9),
            gas_limit BIGINT,
            gas_used BIGINT,
            status VARCHAR(20) DEFAULT 'pending',
            tx_hash VARCHAR(66),
            block_number BIGINT,
            error_message TEXT,
            initiated_by INT NOT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
            FOREIGN KEY (wallet_id) REFERENCES wallets(id),
            FOREIGN KEY (initiated_by) REFERENCES users(id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS audit_logs (
            id INT PRIMARY KEY AUTO_INCREMENT,
            user_id INT,
            action VARCHAR(100) NOT NULL,
            resource VARCHAR(100),
            details JSON,
            ip_address VARCHAR(45),
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (user_id) REFERENCES users(id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Settings table for storing configuration like RPC endpoints
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS settings (
            `key` VARCHAR(100) PRIMARY KEY,
            `value` TEXT NOT NULL,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Orchard sync state table - stores scan progress per wallet
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS orchard_sync_state (
            wallet_id INT PRIMARY KEY,
            last_scanned_height BIGINT UNSIGNED NOT NULL DEFAULT 0,
            notes_found INT UNSIGNED NOT NULL DEFAULT 0,
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
            FOREIGN KEY (wallet_id) REFERENCES wallets(id) ON DELETE CASCADE
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Orchard notes table - stores discovered shielded notes
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS orchard_notes (
            id INT PRIMARY KEY AUTO_INCREMENT,
            wallet_id INT NOT NULL,
            nullifier VARCHAR(64) NOT NULL UNIQUE,
            value_zatoshis BIGINT UNSIGNED NOT NULL,
            block_height BIGINT UNSIGNED NOT NULL,
            tx_hash VARCHAR(64) NOT NULL,
            position_in_block INT UNSIGNED NOT NULL,
            is_spent BOOLEAN DEFAULT FALSE,
            spent_in_tx VARCHAR(64) NULL,
            memo TEXT NULL,
            created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
            FOREIGN KEY (wallet_id) REFERENCES wallets(id) ON DELETE CASCADE,
            INDEX idx_wallet_unspent (wallet_id, is_spent),
            INDEX idx_nullifier (nullifier)
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Add orchard_birthday_height column to wallets table if not exists
    // This stores the block height when wallet was created (for Zcash Orchard scanning)
    let column_exists: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT CAST(COLUMN_NAME AS CHAR) FROM INFORMATION_SCHEMA.COLUMNS
        WHERE TABLE_SCHEMA = DATABASE()
        AND TABLE_NAME = 'wallets'
        AND COLUMN_NAME = 'orchard_birthday_height'
        "#,
    )
    .fetch_optional(pool)
    .await?;

    if column_exists.is_none() {
        sqlx::query(
            "ALTER TABLE wallets ADD COLUMN orchard_birthday_height BIGINT UNSIGNED NULL"
        )
        .execute(pool)
        .await?;
        tracing::info!("Added orchard_birthday_height column to wallets table");

        // Fix historical data: estimate birthday_height from created_at for Zcash wallets
        // Zcash block time is ~75 seconds, Orchard activated at height 1,687,104 (May 31, 2022)
        // Reference point: use a known block height and timestamp
        // Block 2,000,000 was around Oct 2023
        // We'll estimate based on: birthday = reference_height - (reference_time - created_at) / 75
        sqlx::query(
            r#"
            UPDATE wallets
            SET orchard_birthday_height = GREATEST(
                1687104,
                2700000 - FLOOR(TIMESTAMPDIFF(SECOND, created_at, '2025-06-01 00:00:00') / 75)
            )
            WHERE chain = 'zcash' AND orchard_birthday_height IS NULL
            "#,
        )
        .execute(pool)
        .await?;
        tracing::info!("Fixed historical Zcash wallets birthday_height based on created_at");
    }

    // Add spending data columns to orchard_notes table if not exists
    // These fields are required for shielded-to-shielded transfers
    let recipient_column_exists: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT CAST(COLUMN_NAME AS CHAR) FROM INFORMATION_SCHEMA.COLUMNS
        WHERE TABLE_SCHEMA = DATABASE()
        AND TABLE_NAME = 'orchard_notes'
        AND COLUMN_NAME = 'recipient'
        "#,
    )
    .fetch_optional(pool)
    .await?;

    if recipient_column_exists.is_none() {
        sqlx::query(
            r#"
            ALTER TABLE orchard_notes
            ADD COLUMN recipient VARCHAR(128) NULL COMMENT 'Hex-encoded recipient address (43 bytes)',
            ADD COLUMN rho VARCHAR(64) NULL COMMENT 'Hex-encoded rho (32 bytes)',
            ADD COLUMN rseed VARCHAR(64) NULL COMMENT 'Hex-encoded rseed (32 bytes)'
            "#
        )
        .execute(pool)
        .await?;
        tracing::info!("Added spending data columns (recipient, rho, rseed) to orchard_notes table");
    }

    // Add witness data columns to orchard_notes table if not exists
    // These fields store the Merkle witness (auth path) needed to spend notes
    let witness_column_exists: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT CAST(COLUMN_NAME AS CHAR) FROM INFORMATION_SCHEMA.COLUMNS
        WHERE TABLE_SCHEMA = DATABASE()
        AND TABLE_NAME = 'orchard_notes'
        AND COLUMN_NAME = 'witness_root'
        "#,
    )
    .fetch_optional(pool)
    .await?;

    if witness_column_exists.is_none() {
        sqlx::query(
            r#"
            ALTER TABLE orchard_notes
            ADD COLUMN witness_position BIGINT UNSIGNED NULL COMMENT 'Note position in commitment tree',
            ADD COLUMN witness_auth_path TEXT NULL COMMENT 'JSON array of hex-encoded auth path hashes',
            ADD COLUMN witness_root VARCHAR(64) NULL COMMENT 'Hex-encoded tree root (32 bytes)'
            "#
        )
        .execute(pool)
        .await?;
        tracing::info!("Added witness data columns to orchard_notes table");
    }

    // Add last_witness_height column to orchard_sync_state table if not exists
    // This tracks when witness data was last updated, allowing lazy witness sync
    let witness_height_column_exists: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT CAST(COLUMN_NAME AS CHAR) FROM INFORMATION_SCHEMA.COLUMNS
        WHERE TABLE_SCHEMA = DATABASE()
        AND TABLE_NAME = 'orchard_sync_state'
        AND COLUMN_NAME = 'last_witness_height'
        "#,
    )
    .fetch_optional(pool)
    .await?;

    if witness_height_column_exists.is_none() {
        sqlx::query(
            r#"
            ALTER TABLE orchard_sync_state
            ADD COLUMN last_witness_height BIGINT UNSIGNED NOT NULL DEFAULT 0
                COMMENT 'Block height when witnesses were last updated'
            "#
        )
        .execute(pool)
        .await?;
        tracing::info!("Added last_witness_height column to orchard_sync_state table");
    }

    // Expand from_address and to_address columns in transfers table for Zcash Unified Addresses
    // Zcash UA can be 250+ characters, VARCHAR(42) was for Ethereum addresses only
    let address_col_info: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT CAST(DATA_TYPE AS CHAR) FROM INFORMATION_SCHEMA.COLUMNS
        WHERE TABLE_SCHEMA = DATABASE()
        AND TABLE_NAME = 'transfers'
        AND COLUMN_NAME = 'from_address'
        AND CHARACTER_MAXIMUM_LENGTH < 512
        "#,
    )
    .fetch_optional(pool)
    .await?;

    if address_col_info.is_some() {
        sqlx::query(
            "ALTER TABLE transfers MODIFY COLUMN from_address VARCHAR(512) NOT NULL"
        )
        .execute(pool)
        .await?;
        sqlx::query(
            "ALTER TABLE transfers MODIFY COLUMN to_address VARCHAR(512) NOT NULL"
        )
        .execute(pool)
        .await?;
        tracing::info!("Expanded from_address and to_address columns to VARCHAR(512) for Zcash addresses");
    }

    // Create orchard_tree_state table for storing global commitment tree state
    // This enables incremental witness sync without rescanning from note birth height
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS orchard_tree_state (
            id INT PRIMARY KEY DEFAULT 1,
            tree_data MEDIUMBLOB NOT NULL COMMENT 'Serialized CommitmentTree frontier',
            tree_height BIGINT UNSIGNED NOT NULL COMMENT 'Block height of tree state',
            tree_size BIGINT UNSIGNED NOT NULL COMMENT 'Number of commitments in tree',
            updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP
        )
        "#,
    )
    .execute(pool)
    .await?;

    // Add witness_state column to orchard_notes for incremental witness sync
    // This stores serialized IncrementalWitness that can be updated incrementally
    let witness_state_column_exists: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT CAST(COLUMN_NAME AS CHAR) FROM INFORMATION_SCHEMA.COLUMNS
        WHERE TABLE_SCHEMA = DATABASE()
        AND TABLE_NAME = 'orchard_notes'
        AND COLUMN_NAME = 'witness_state'
        "#,
    )
    .fetch_optional(pool)
    .await?;

    if witness_state_column_exists.is_none() {
        sqlx::query(
            r#"
            ALTER TABLE orchard_notes
            ADD COLUMN witness_state MEDIUMBLOB NULL
                COMMENT 'Serialized IncrementalWitness for incremental updates'
            "#
        )
        .execute(pool)
        .await?;
        tracing::info!("Added witness_state column to orchard_notes table for incremental sync");
    }

    // F4.0-c dual-pool: tag each Orchard note with the shielded pool it lives in
    // (old Orchard pool vs Ironwood/NU6.3). All existing notes predate NU6.3, so
    // the NOT NULL DEFAULT 'orchard' backfills them correctly; the dual-pool
    // scanner will tag freshly-scanned Ironwood (V3) notes as 'ironwood'. Additive
    // and backward-compatible: inserts that don't set it default to 'orchard'.
    let pool_column_exists: Option<(String,)> = sqlx::query_as(
        r#"
        SELECT CAST(COLUMN_NAME AS CHAR) FROM INFORMATION_SCHEMA.COLUMNS
        WHERE TABLE_SCHEMA = DATABASE()
        AND TABLE_NAME = 'orchard_notes'
        AND COLUMN_NAME = 'pool'
        "#,
    )
    .fetch_optional(pool)
    .await?;

    if pool_column_exists.is_none() {
        sqlx::query(
            r#"
            ALTER TABLE orchard_notes
            ADD COLUMN pool VARCHAR(16) NOT NULL DEFAULT 'orchard'
                COMMENT 'Shielded pool: orchard (old, V2) or ironwood (NU6.3, V3)'
            "#,
        )
        .execute(pool)
        .await?;
        tracing::info!(
            "Added pool column to orchard_notes (existing notes backfilled to 'orchard')"
        );
    }

    // M1 (2026-06) migrations: F1.1 viewing-key audit, F2.1 maker/checker,
    // F3.1 payroll run.  All additive — does not modify existing schema.
    migrations_m1::run_m1_migrations(pool).await?;

    // F4 (2026-07) migrations: F4.1 Ironwood migration runs, F4.2 batch
    // privacy transfers.  All additive — does not modify existing schema.
    migrations_f4::run_f4_migrations(pool).await?;

    tracing::info!("Database migrations completed successfully");
    Ok(())
}
