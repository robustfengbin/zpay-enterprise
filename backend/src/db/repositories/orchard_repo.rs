//! Orchard notes and sync state repository
//!
//! Provides persistence for Orchard scan state and discovered notes.
//! Some methods are reserved for future use (e.g., mark_note_spent).

#![allow(dead_code)]

use crate::error::AppResult;
use sqlx::MySqlPool;

/// Stored Orchard note from database
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct StoredOrchardNote {
    pub id: i32,
    pub wallet_id: i32,
    pub nullifier: String,
    pub value_zatoshis: u64,
    pub block_height: u64,
    pub tx_hash: String,
    pub position_in_block: u32,
    pub is_spent: bool,
    pub spent_in_tx: Option<String>,
    pub memo: Option<String>,
    // Spending data (for shielded-to-shielded transfers)
    pub recipient: Option<String>,  // Hex-encoded 43 bytes
    pub rho: Option<String>,        // Hex-encoded 32 bytes
    pub rseed: Option<String>,      // Hex-encoded 32 bytes
    // Witness data for spending (JSON-serialized)
    pub witness_position: Option<u64>,
    pub witness_auth_path: Option<String>, // JSON array of hex-encoded 32-byte hashes
    pub witness_root: Option<String>,      // Hex-encoded 32-byte root
}

/// Sync state for a wallet
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct OrchardSyncState {
    pub wallet_id: i32,
    pub last_scanned_height: u64,
    pub notes_found: u32,
    /// Block height when witness data was last updated
    /// Used to determine if witnesses need refresh (lazy sync)
    pub last_witness_height: u64,
}

/// Global tree state for incremental witness sync
#[derive(Debug, Clone)]
pub struct OrchardTreeState {
    pub tree_data: Vec<u8>,
    pub tree_height: u64,
    pub tree_size: u64,
}

/// Note info with witness state for incremental sync
#[derive(Debug, Clone)]
pub struct NoteWitnessInfo {
    pub nullifier: String,
    pub block_height: u64,
    pub witness_position: Option<u64>,
    pub witness_state: Option<Vec<u8>>,
}

#[derive(Clone)]
pub struct OrchardRepository {
    pool: MySqlPool,
}

impl OrchardRepository {
    pub fn new(pool: MySqlPool) -> Self {
        Self { pool }
    }

    /// Get a reference to the connection pool for monitoring
    pub fn pool(&self) -> &MySqlPool {
        &self.pool
    }

    // =========================================================================
    // Sync State Operations
    // =========================================================================

    /// Get sync state for a wallet
    pub async fn get_sync_state(&self, wallet_id: i32) -> AppResult<Option<OrchardSyncState>> {
        let state = sqlx::query_as::<_, OrchardSyncState>(
            "SELECT wallet_id, last_scanned_height, notes_found, last_witness_height FROM orchard_sync_state WHERE wallet_id = ?"
        )
        .bind(wallet_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(state)
    }

    /// Get minimum last_scanned_height across all wallets (for global sync)
    pub async fn get_min_scanned_height(&self) -> AppResult<u64> {
        let result: Option<(u64,)> = sqlx::query_as(
            "SELECT MIN(last_scanned_height) FROM orchard_sync_state"
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(result.map(|(h,)| h).unwrap_or(0))
    }

    /// Update or insert sync state for a wallet
    pub async fn upsert_sync_state(&self, wallet_id: i32, last_scanned_height: u64, notes_found: u32) -> AppResult<()> {
        sqlx::query(
            r#"
            INSERT INTO orchard_sync_state (wallet_id, last_scanned_height, notes_found)
            VALUES (?, ?, ?)
            ON DUPLICATE KEY UPDATE
                last_scanned_height = VALUES(last_scanned_height),
                notes_found = VALUES(notes_found)
            "#
        )
        .bind(wallet_id)
        .bind(last_scanned_height)
        .bind(notes_found)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Update witness height for a wallet (called after persist_witnesses)
    pub async fn update_witness_height(&self, wallet_id: i32, witness_height: u64) -> AppResult<()> {
        sqlx::query(
            "UPDATE orchard_sync_state SET last_witness_height = ? WHERE wallet_id = ?"
        )
        .bind(witness_height)
        .bind(wallet_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Get minimum last_witness_height across all wallets with unspent notes
    pub async fn get_min_witness_height(&self) -> AppResult<u64> {
        let result: Option<(u64,)> = sqlx::query_as(
            r#"
            SELECT MIN(s.last_witness_height) FROM orchard_sync_state s
            INNER JOIN orchard_notes n ON s.wallet_id = n.wallet_id
            WHERE n.is_spent = FALSE
            "#
        )
        .fetch_optional(&self.pool)
        .await?;
        Ok(result.map(|(h,)| h).unwrap_or(0))
    }

    /// Batch update sync state for multiple wallets
    pub async fn batch_update_sync_height(&self, wallet_ids: &[i32], height: u64) -> AppResult<()> {
        if wallet_ids.is_empty() {
            return Ok(());
        }

        // Build placeholders for IN clause
        let placeholders: Vec<String> = wallet_ids.iter().map(|_| "?".to_string()).collect();
        let query = format!(
            "UPDATE orchard_sync_state SET last_scanned_height = ? WHERE wallet_id IN ({})",
            placeholders.join(",")
        );

        let mut q = sqlx::query(&query).bind(height);
        for id in wallet_ids {
            q = q.bind(*id);
        }
        q.execute(&self.pool).await?;
        Ok(())
    }

    // =========================================================================
    // Notes Operations
    // =========================================================================

    /// Save a newly discovered note
    pub async fn save_note(
        &self,
        wallet_id: i32,
        nullifier: &str,
        value_zatoshis: u64,
        block_height: u64,
        tx_hash: &str,
        position_in_block: u32,
        memo: Option<&str>,
    ) -> AppResult<i32> {
        let result = sqlx::query(
            r#"
            INSERT INTO orchard_notes
                (wallet_id, nullifier, value_zatoshis, block_height, tx_hash, position_in_block, memo)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            ON DUPLICATE KEY UPDATE id = id
            "#
        )
        .bind(wallet_id)
        .bind(nullifier)
        .bind(value_zatoshis)
        .bind(block_height)
        .bind(tx_hash)
        .bind(position_in_block)
        .bind(memo)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_id() as i32)
    }

    /// Save a newly discovered note with full spending data and witness position
    ///
    /// The witness_position is the global position in the Orchard commitment tree,
    /// saved at discovery time to enable fast witness refresh without re-scanning.
    pub async fn save_note_full(
        &self,
        wallet_id: i32,
        nullifier: &str,
        value_zatoshis: u64,
        block_height: u64,
        tx_hash: &str,
        position_in_block: u32,
        memo: Option<&str>,
        recipient: &str,
        rho: &str,
        rseed: &str,
        witness_position: u64,  // Global tree position, saved at discovery
    ) -> AppResult<i32> {
        let result = sqlx::query(
            r#"
            INSERT INTO orchard_notes
                (wallet_id, nullifier, value_zatoshis, block_height, tx_hash, position_in_block, memo, recipient, rho, rseed, witness_position)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON DUPLICATE KEY UPDATE
                recipient = VALUES(recipient),
                rho = VALUES(rho),
                rseed = VALUES(rseed),
                witness_position = VALUES(witness_position)
            "#
        )
        .bind(wallet_id)
        .bind(nullifier)
        .bind(value_zatoshis)
        .bind(block_height)
        .bind(tx_hash)
        .bind(position_in_block)
        .bind(memo)
        .bind(recipient)
        .bind(rho)
        .bind(rseed)
        .bind(witness_position)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_id() as i32)
    }

    /// Batch save multiple notes
    pub async fn save_notes_batch(&self, notes: &[(i32, String, u64, u64, String, u32, Option<String>)]) -> AppResult<usize> {
        if notes.is_empty() {
            return Ok(0);
        }

        let mut saved = 0;
        for (wallet_id, nullifier, value, height, tx_hash, position, memo) in notes {
            if self.save_note(*wallet_id, nullifier, *value, *height, tx_hash, *position, memo.as_deref()).await.is_ok() {
                saved += 1;
            }
        }
        Ok(saved)
    }

    /// Get all unspent notes for a wallet
    // -----------------------------------------------------------------
    // F1.1 disclosure queries — include spent notes (audit needs history),
    // bounded by tx / wallet / block-height range.  Witness columns are
    // not needed by the disclosure body and would inflate payload size.
    // -----------------------------------------------------------------

    pub async fn list_notes_by_tx_hash(
        &self,
        wallet_id: i32,
        tx_hash: &str,
    ) -> AppResult<Vec<StoredOrchardNote>> {
        let notes = sqlx::query_as::<_, StoredOrchardNote>(
            r#"
            SELECT id, wallet_id, nullifier, value_zatoshis, block_height, tx_hash,
                   position_in_block, is_spent, spent_in_tx, memo, recipient, rho, rseed,
                   witness_position, witness_auth_path, witness_root
            FROM orchard_notes
            WHERE wallet_id = ? AND tx_hash = ?
            ORDER BY position_in_block ASC
            "#,
        )
        .bind(wallet_id)
        .bind(tx_hash)
        .fetch_all(&self.pool)
        .await?;
        Ok(notes)
    }

    pub async fn list_all_notes_by_wallet(
        &self,
        wallet_id: i32,
    ) -> AppResult<Vec<StoredOrchardNote>> {
        let notes = sqlx::query_as::<_, StoredOrchardNote>(
            r#"
            SELECT id, wallet_id, nullifier, value_zatoshis, block_height, tx_hash,
                   position_in_block, is_spent, spent_in_tx, memo, recipient, rho, rseed,
                   witness_position, witness_auth_path, witness_root
            FROM orchard_notes
            WHERE wallet_id = ?
            ORDER BY block_height ASC, position_in_block ASC
            "#,
        )
        .bind(wallet_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(notes)
    }

    pub async fn list_notes_in_height_range(
        &self,
        wallet_id: i32,
        from_height: u64,
        to_height: u64,
    ) -> AppResult<Vec<StoredOrchardNote>> {
        let notes = sqlx::query_as::<_, StoredOrchardNote>(
            r#"
            SELECT id, wallet_id, nullifier, value_zatoshis, block_height, tx_hash,
                   position_in_block, is_spent, spent_in_tx, memo, recipient, rho, rseed,
                   witness_position, witness_auth_path, witness_root
            FROM orchard_notes
            WHERE wallet_id = ? AND block_height >= ? AND block_height <= ?
            ORDER BY block_height ASC, position_in_block ASC
            "#,
        )
        .bind(wallet_id)
        .bind(from_height)
        .bind(to_height)
        .fetch_all(&self.pool)
        .await?;
        Ok(notes)
    }

    pub async fn get_unspent_notes(&self, wallet_id: i32) -> AppResult<Vec<StoredOrchardNote>> {
        let notes = sqlx::query_as::<_, StoredOrchardNote>(
            r#"
            SELECT id, wallet_id, nullifier, value_zatoshis, block_height, tx_hash,
                   position_in_block, is_spent, spent_in_tx, memo, recipient, rho, rseed,
                   witness_position, witness_auth_path, witness_root
            FROM orchard_notes
            WHERE wallet_id = ? AND is_spent = FALSE
            ORDER BY block_height ASC
            "#
        )
        .bind(wallet_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(notes)
    }

    /// Get unspent notes with spending data (for shielded transfers)
    pub async fn get_spendable_notes(&self, wallet_id: i32) -> AppResult<Vec<StoredOrchardNote>> {
        let notes = sqlx::query_as::<_, StoredOrchardNote>(
            r#"
            SELECT id, wallet_id, nullifier, value_zatoshis, block_height, tx_hash,
                   position_in_block, is_spent, spent_in_tx, memo, recipient, rho, rseed,
                   witness_position, witness_auth_path, witness_root
            FROM orchard_notes
            WHERE wallet_id = ? AND is_spent = FALSE
              AND recipient IS NOT NULL AND rho IS NOT NULL AND rseed IS NOT NULL
            ORDER BY value_zatoshis DESC
            "#
        )
        .bind(wallet_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(notes)
    }

    /// Get spendable notes with witness data
    pub async fn get_notes_with_witnesses(&self, wallet_id: i32) -> AppResult<Vec<StoredOrchardNote>> {
        let notes = sqlx::query_as::<_, StoredOrchardNote>(
            r#"
            SELECT id, wallet_id, nullifier, value_zatoshis, block_height, tx_hash,
                   position_in_block, is_spent, spent_in_tx, memo, recipient, rho, rseed,
                   witness_position, witness_auth_path, witness_root
            FROM orchard_notes
            WHERE wallet_id = ? AND is_spent = FALSE
              AND recipient IS NOT NULL AND rho IS NOT NULL AND rseed IS NOT NULL
              AND witness_auth_path IS NOT NULL AND witness_root IS NOT NULL
            ORDER BY value_zatoshis DESC
            "#
        )
        .bind(wallet_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(notes)
    }

    /// Update witness data for a note
    pub async fn update_witness_data(
        &self,
        note_id: i32,
        position: u64,
        auth_path: &str,  // JSON array of hex strings
        root: &str,       // Hex-encoded root
    ) -> AppResult<bool> {
        let result = sqlx::query(
            r#"
            UPDATE orchard_notes
            SET witness_position = ?, witness_auth_path = ?, witness_root = ?
            WHERE id = ?
            "#
        )
        .bind(position)
        .bind(auth_path)
        .bind(root)
        .bind(note_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Update witness data by nullifier
    pub async fn update_witness_by_nullifier(
        &self,
        nullifier: &str,
        position: u64,
        auth_path: &str,
        root: &str,
    ) -> AppResult<bool> {
        let result = sqlx::query(
            r#"
            UPDATE orchard_notes
            SET witness_position = ?, witness_auth_path = ?, witness_root = ?
            WHERE nullifier = ?
            "#
        )
        .bind(position)
        .bind(auth_path)
        .bind(root)
        .bind(nullifier)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Get total unspent balance for a wallet
    pub async fn get_balance(&self, wallet_id: i32) -> AppResult<u64> {
        // CAST to UNSIGNED because SUM returns DECIMAL which isn't compatible with i64
        let result: Option<(u64,)> = sqlx::query_as(
            "SELECT CAST(COALESCE(SUM(value_zatoshis), 0) AS UNSIGNED) FROM orchard_notes WHERE wallet_id = ? AND is_spent = FALSE"
        )
        .bind(wallet_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(result.map(|(b,)| b).unwrap_or(0))
    }

    /// Mark a note as spent
    pub async fn mark_note_spent(&self, nullifier: &str, spent_in_tx: &str) -> AppResult<bool> {
        let result = sqlx::query(
            "UPDATE orchard_notes SET is_spent = TRUE, spent_in_tx = ? WHERE nullifier = ? AND is_spent = FALSE"
        )
        .bind(spent_in_tx)
        .bind(nullifier)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Check if a nullifier exists (note was spent)
    pub async fn nullifier_exists(&self, nullifier: &str) -> AppResult<bool> {
        let result: Option<(i32,)> = sqlx::query_as(
            "SELECT 1 FROM orchard_notes WHERE nullifier = ? LIMIT 1"
        )
        .bind(nullifier)
        .fetch_optional(&self.pool)
        .await?;
        Ok(result.is_some())
    }

    /// Get unspent notes count for a wallet
    pub async fn get_notes_count(&self, wallet_id: i32) -> AppResult<u32> {
        let result: Option<(i64,)> = sqlx::query_as(
            "SELECT COUNT(*) FROM orchard_notes WHERE wallet_id = ? AND is_spent = FALSE"
        )
        .bind(wallet_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(result.map(|(c,)| c as u32).unwrap_or(0))
    }

    /// Check if wallet has unspent notes missing witness data
    /// Returns true if there are notes that need witness refresh
    pub async fn has_notes_missing_witness(&self, wallet_id: i32) -> AppResult<bool> {
        let result: Option<(i64,)> = sqlx::query_as(
            r#"
            SELECT COUNT(*) FROM orchard_notes
            WHERE wallet_id = ?
              AND is_spent = FALSE
              AND (witness_position IS NULL OR witness_auth_path IS NULL OR witness_root IS NULL)
            "#
        )
        .bind(wallet_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(result.map(|(c,)| c > 0).unwrap_or(false))
    }

    // =========================================================================
    // Tree State Operations (for incremental witness sync)
    // =========================================================================

    /// Save global tree state
    /// Uses REPLACE INTO to ensure only one row exists
    pub async fn save_tree_state(&self, tree_data: &[u8], tree_height: u64, tree_size: u64) -> AppResult<()> {
        // First delete any existing rows, then insert new one
        // This ensures we always have exactly one row regardless of id
        sqlx::query("DELETE FROM orchard_tree_state")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            "INSERT INTO orchard_tree_state (id, tree_data, tree_height, tree_size) VALUES (1, ?, ?, ?)"
        )
        .bind(tree_data)
        .bind(tree_height)
        .bind(tree_size)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Load global tree state (only one row in table)
    pub async fn load_tree_state(&self) -> AppResult<Option<OrchardTreeState>> {
        let result: Option<(Vec<u8>, u64, u64)> = sqlx::query_as(
            "SELECT tree_data, tree_height, tree_size FROM orchard_tree_state LIMIT 1"
        )
        .fetch_optional(&self.pool)
        .await?;

        Ok(result.map(|(tree_data, tree_height, tree_size)| OrchardTreeState {
            tree_data,
            tree_height,
            tree_size,
        }))
    }

    /// Delete tree state (for reset/rebuild)
    pub async fn delete_tree_state(&self) -> AppResult<()> {
        sqlx::query("DELETE FROM orchard_tree_state WHERE id = 1")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    // =========================================================================
    // Witness State Operations (for incremental witness sync)
    // =========================================================================

    /// Save witness state for a note (by nullifier)
    pub async fn save_witness_state(&self, nullifier: &str, witness_state: &[u8]) -> AppResult<bool> {
        let result = sqlx::query(
            "UPDATE orchard_notes SET witness_state = ? WHERE nullifier = ?"
        )
        .bind(witness_state)
        .bind(nullifier)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    /// Load witness states for all unspent notes of specified wallets
    pub async fn load_witness_states(&self, wallet_ids: &[i32]) -> AppResult<Vec<NoteWitnessInfo>> {
        if wallet_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Build placeholders for IN clause
        let placeholders: Vec<String> = wallet_ids.iter().map(|_| "?".to_string()).collect();
        let query = format!(
            r#"
            SELECT nullifier, block_height, witness_position, witness_state
            FROM orchard_notes
            WHERE wallet_id IN ({}) AND is_spent = FALSE
            ORDER BY block_height ASC
            "#,
            placeholders.join(",")
        );

        let mut q = sqlx::query_as::<_, (String, u64, Option<u64>, Option<Vec<u8>>)>(&query);
        for id in wallet_ids {
            q = q.bind(*id);
        }

        let rows = q.fetch_all(&self.pool).await?;

        Ok(rows.into_iter().map(|(nullifier, block_height, witness_position, witness_state)| {
            NoteWitnessInfo {
                nullifier,
                block_height,
                witness_position,
                witness_state,
            }
        }).collect())
    }

    /// Get minimum block height among all unspent notes for specified wallets
    pub async fn get_min_note_height(&self, wallet_ids: &[i32]) -> AppResult<Option<u64>> {
        if wallet_ids.is_empty() {
            return Ok(None);
        }

        let placeholders: Vec<String> = wallet_ids.iter().map(|_| "?".to_string()).collect();
        let query = format!(
            "SELECT MIN(block_height) FROM orchard_notes WHERE wallet_id IN ({}) AND is_spent = FALSE",
            placeholders.join(",")
        );

        let mut q = sqlx::query_as::<_, (Option<u64>,)>(&query);
        for id in wallet_ids {
            q = q.bind(*id);
        }

        let result = q.fetch_optional(&self.pool).await?;
        Ok(result.and_then(|(h,)| h))
    }

    /// Get notes that have witness_position but no witness_state
    /// These notes need to be rescanned from their block_height to build proper witnesses
    pub async fn get_notes_without_witness_state(&self, wallet_ids: &[i32]) -> AppResult<Vec<NoteWitnessInfo>> {
        if wallet_ids.is_empty() {
            return Ok(Vec::new());
        }

        let placeholders: Vec<String> = wallet_ids.iter().map(|_| "?".to_string()).collect();
        let query = format!(
            r#"
            SELECT nullifier, block_height, witness_position, witness_state
            FROM orchard_notes
            WHERE wallet_id IN ({}) AND is_spent = FALSE
              AND witness_position IS NOT NULL
              AND witness_state IS NULL
            ORDER BY block_height ASC
            "#,
            placeholders.join(",")
        );

        let mut q = sqlx::query_as::<_, (String, u64, Option<u64>, Option<Vec<u8>>)>(&query);
        for id in wallet_ids {
            q = q.bind(*id);
        }

        let rows = q.fetch_all(&self.pool).await?;

        Ok(rows.into_iter().map(|(nullifier, block_height, witness_position, witness_state)| {
            NoteWitnessInfo {
                nullifier,
                block_height,
                witness_position,
                witness_state,
            }
        }).collect())
    }

    /// Get minimum block_height of notes without witness_state
    pub async fn get_min_height_notes_without_witness_state(&self, wallet_ids: &[i32]) -> AppResult<Option<u64>> {
        if wallet_ids.is_empty() {
            return Ok(None);
        }

        let placeholders: Vec<String> = wallet_ids.iter().map(|_| "?".to_string()).collect();
        let query = format!(
            r#"
            SELECT MIN(block_height) FROM orchard_notes
            WHERE wallet_id IN ({}) AND is_spent = FALSE
              AND witness_position IS NOT NULL
              AND witness_state IS NULL
            "#,
            placeholders.join(",")
        );

        let mut q = sqlx::query_as::<_, (Option<u64>,)>(&query);
        for id in wallet_ids {
            q = q.bind(*id);
        }

        let result = q.fetch_optional(&self.pool).await?;
        Ok(result.and_then(|(h,)| h))
    }

    /// Batch save witness states for multiple notes
    pub async fn batch_save_witness_states(&self, updates: &[(String, Vec<u8>)]) -> AppResult<usize> {
        if updates.is_empty() {
            return Ok(0);
        }

        let mut saved = 0;
        for (nullifier, witness_state) in updates {
            if self.save_witness_state(nullifier, witness_state).await? {
                saved += 1;
            }
        }
        Ok(saved)
    }

    /// Update both witness result (auth_path, root) and witness state for a note
    pub async fn update_witness_full(
        &self,
        nullifier: &str,
        position: u64,
        auth_path: &str,
        root: &str,
        witness_state: &[u8],
    ) -> AppResult<bool> {
        let result = sqlx::query(
            r#"
            UPDATE orchard_notes
            SET witness_position = ?, witness_auth_path = ?, witness_root = ?, witness_state = ?
            WHERE nullifier = ?
            "#
        )
        .bind(position)
        .bind(auth_path)
        .bind(root)
        .bind(witness_state)
        .bind(nullifier)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }
}
