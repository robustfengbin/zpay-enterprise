//! Incremental witness sync module for Orchard notes
//!
//! This module implements efficient incremental witness synchronization:
//! - Saves and restores tree/witness states to avoid rescanning from note birth height
//! - Updates witnesses incrementally as new blocks arrive
//! - Provides ready-to-use witnesses for spending

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::db::repositories::orchard_repo::OrchardRepository;

use super::keys::OrchardViewingKey;
use super::scanner::{CompactBlock, CompactOrchardAction, OrchardNote};
use super::tree::{OrchardTreeTracker, WitnessData, ORCHARD_TREE_DEPTH};
use super::{OrchardError, OrchardResult};

use incrementalmerkletree::witness::IncrementalWitness;
use orchard::tree::MerkleHashOrchard;

/// Witness sync manager for incremental updates
pub struct WitnessSyncManager {
    /// Tree tracker (shared with scanner for unified state)
    tree: Arc<RwLock<OrchardTreeTracker>>,

    /// Database repository
    db_repo: Arc<OrchardRepository>,

    /// Viewing keys by wallet_id
    viewing_keys: Arc<RwLock<HashMap<i32, OrchardViewingKey>>>,

    /// Zcash RPC client
    rpc_client: Arc<reqwest::Client>,
    rpc_url: String,
    rpc_user: String,
    rpc_password: String,

    /// Witnesses keyed by nullifier (hex string)
    /// Stored separately for efficient access during sync
    witnesses: Arc<RwLock<HashMap<String, IncrementalWitness<MerkleHashOrchard, ORCHARD_TREE_DEPTH>>>>,

    /// Map nullifier -> position for quick lookup
    nullifier_positions: Arc<RwLock<HashMap<String, u64>>>,

    /// Cached Orchard (NU5) activation height, read live from the node so scan
    /// floors match the network (regtest/testnet chains activate far below the
    /// mainnet 1,687,104). Populated lazily on first use.
    orchard_activation_cache: Arc<RwLock<Option<u64>>>,
}

impl WitnessSyncManager {
    /// Create a new witness sync manager
    pub fn new(
        db_repo: Arc<OrchardRepository>,
        rpc_url: String,
        rpc_user: String,
        rpc_password: String,
    ) -> Self {
        Self {
            tree: Arc::new(RwLock::new(OrchardTreeTracker::new())),
            db_repo,
            viewing_keys: Arc::new(RwLock::new(HashMap::new())),
            rpc_client: Arc::new(reqwest::Client::new()),
            rpc_url,
            rpc_user,
            rpc_password,
            witnesses: Arc::new(RwLock::new(HashMap::new())),
            nullifier_positions: Arc::new(RwLock::new(HashMap::new())),
            orchard_activation_cache: Arc::new(RwLock::new(None)),
        }
    }

    /// Resolve (and cache) the Orchard (NU5) activation height from the node's
    /// getblockchaininfo `upgrades`. Scan/frontier floors use this so short
    /// regtest/testnet chains don't request a nonexistent block. Falls back to
    /// the mainnet constant if the node omits the upgrades table.
    async fn orchard_activation_height(&self) -> u64 {
        if let Some(h) = *self.orchard_activation_cache.read().await {
            return h;
        }
        let h = self.fetch_nu5_activation_height().await.unwrap_or(1_687_104);
        *self.orchard_activation_cache.write().await = Some(h);
        h
    }

    /// Read the NU5 activation height from getblockchaininfo `upgrades`.
    async fn fetch_nu5_activation_height(&self) -> Option<u64> {
        let request = serde_json::json!({
            "jsonrpc": "1.0",
            "id": "witness_sync",
            "method": "getblockchaininfo",
            "params": []
        });

        let response = self.rpc_client
            .post(&self.rpc_url)
            .basic_auth(&self.rpc_user, Some(&self.rpc_password))
            .json(&request)
            .send()
            .await
            .ok()?;

        let result: serde_json::Value = response.json().await.ok()?;
        let upgrades = result["result"]["upgrades"].as_object()?;
        for (_branch_id, entry) in upgrades {
            let is_nu5 = entry["name"]
                .as_str()
                .map(|s| s.eq_ignore_ascii_case("nu5"))
                .unwrap_or(false);
            if is_nu5 {
                return entry["activationheight"].as_u64();
            }
        }
        None
    }

    /// Register a viewing key for a wallet
    pub async fn register_wallet(&self, wallet_id: i32, mut viewing_key: OrchardViewingKey) {
        // Set the wallet_id on the viewing key so discovered notes have correct wallet_id
        viewing_key.wallet_id = Some(wallet_id);
        let mut keys = self.viewing_keys.write().await;
        keys.insert(wallet_id, viewing_key);
        tracing::info!("[WitnessSync] Registered wallet {}", wallet_id);
    }

    /// Get registered wallet IDs
    pub async fn get_wallet_ids(&self) -> Vec<i32> {
        let keys = self.viewing_keys.read().await;
        keys.keys().copied().collect()
    }

    /// Initialize or restore state from database
    ///
    /// This should be called once at startup or when starting a sync cycle.
    /// Returns the tree height (last synced block) or 0 if no state exists.
    pub async fn initialize(&self) -> OrchardResult<u64> {
        // Try to load existing tree state
        let tree_state = self.db_repo.load_tree_state().await
            .map_err(|e| OrchardError::DatabaseError(e.to_string()))?;

        let wallet_ids = self.get_wallet_ids().await;

        if let Some(state) = tree_state {
            // Restore tree from saved state
            let mut tree = self.tree.write().await;
            *tree = OrchardTreeTracker::from_serialized(
                &state.tree_data,
                state.tree_size,
                state.tree_height,
            )?;

            tracing::info!(
                "[WitnessSync] Restored tree state: height={}, size={}",
                state.tree_height,
                state.tree_size
            );

            // Load witness states for all unspent notes
            let note_infos = self.db_repo.load_witness_states(&wallet_ids).await
                .map_err(|e| OrchardError::DatabaseError(e.to_string()))?;

            let mut witnesses = self.witnesses.write().await;
            let mut positions = self.nullifier_positions.write().await;

            for info in note_infos {
                if let (Some(pos), Some(ws_data)) = (info.witness_position, info.witness_state) {
                    match OrchardTreeTracker::deserialize_witness(&ws_data) {
                        Ok(witness) => {
                            witnesses.insert(info.nullifier.clone(), witness);
                            positions.insert(info.nullifier, pos);
                        }
                        Err(e) => {
                            tracing::warn!(
                                "[WitnessSync] Failed to deserialize witness: {}",
                                e
                            );
                        }
                    }
                }
            }

            tracing::info!(
                "[WitnessSync] Loaded {} witness states",
                witnesses.len()
            );

            Ok(state.tree_height)
        } else {
            // No saved state, need to initialize from frontier
            tracing::info!("[WitnessSync] No saved tree state, will initialize from frontier");
            Ok(0)
        }
    }

    /// Initialize tree from frontier (when no saved state exists)
    ///
    /// `frontier_height` should be the block height before the earliest note
    pub async fn init_from_frontier(&self, frontier_height: u64) -> OrchardResult<()> {
        let (frontier_hex, tree_size, _root) = self.get_tree_state(frontier_height).await?;

        let mut tree = self.tree.write().await;
        tree.reset_from_frontier(&frontier_hex, tree_size, frontier_height)?;

        tracing::info!(
            "[WitnessSync] Initialized from frontier: height={}, size={}",
            frontier_height,
            tree_size
        );

        Ok(())
    }

    /// Get tree state from RPC
    async fn get_tree_state(&self, height: u64) -> OrchardResult<(String, u64, String)> {
        let request = serde_json::json!({
            "jsonrpc": "1.0",
            "id": "witness_sync",
            "method": "z_gettreestate",
            "params": [height.to_string()]
        });

        let response = self.rpc_client
            .post(&self.rpc_url)
            .basic_auth(&self.rpc_user, Some(&self.rpc_password))
            .json(&request)
            .send()
            .await
            .map_err(|e| OrchardError::RpcError(format!("RPC request failed: {}", e)))?;

        let result: serde_json::Value = response.json().await
            .map_err(|e| OrchardError::RpcError(format!("Failed to parse RPC response: {}", e)))?;

        if let Some(error) = result.get("error").and_then(|e| e.as_object()) {
            let msg = error.get("message").and_then(|m| m.as_str()).unwrap_or("Unknown error");
            return Err(OrchardError::RpcError(format!("RPC error: {}", msg)));
        }

        let orchard = result["result"]["orchard"].as_object()
            .ok_or_else(|| OrchardError::RpcError("Missing orchard field".to_string()))?;

        let frontier = orchard.get("commitments")
            .and_then(|c| c.get("finalState"))
            .and_then(|f| f.as_str())
            .ok_or_else(|| OrchardError::RpcError("Missing frontier".to_string()))?;

        let tree_size = orchard.get("commitments")
            .and_then(|c| c.get("finalPosition"))
            .and_then(|p| p.as_u64())
            .unwrap_or(0);

        let root = orchard.get("root")
            .and_then(|r| r.as_str())
            .unwrap_or("")
            .to_string();

        Ok((frontier.to_string(), tree_size, root))
    }

    /// Get current chain height from RPC
    pub async fn get_chain_height(&self) -> OrchardResult<u64> {
        let request = serde_json::json!({
            "jsonrpc": "1.0",
            "id": "witness_sync",
            "method": "getblockcount",
            "params": []
        });

        let response = self.rpc_client
            .post(&self.rpc_url)
            .basic_auth(&self.rpc_user, Some(&self.rpc_password))
            .json(&request)
            .send()
            .await
            .map_err(|e| OrchardError::RpcError(format!("RPC request failed: {}", e)))?;

        let result: serde_json::Value = response.json().await
            .map_err(|e| OrchardError::RpcError(format!("Failed to parse response: {}", e)))?;

        result["result"].as_u64()
            .ok_or_else(|| OrchardError::RpcError("Invalid block count".to_string()))
    }

    /// Process a batch of blocks and update witnesses
    ///
    /// This is the core incremental sync function:
    /// 1. For each commitment in the blocks:
    ///    - Update all existing witnesses
    ///    - If it matches a known position (from DB), mark it
    ///    - Try to decrypt and discover new notes
    /// 2. Save updated states to database
    ///
    /// Returns the number of new notes found
    pub async fn process_blocks(
        &self,
        blocks: Vec<CompactBlock>,
        known_positions: &HashMap<u64, String>,  // position -> nullifier
    ) -> OrchardResult<Vec<OrchardNote>> {
        if blocks.is_empty() {
            return Ok(Vec::new());
        }

        let first_height = blocks.first().map(|b| b.height).unwrap_or(0);
        let last_height = blocks.last().map(|b| b.height).unwrap_or(0);
        let viewing_keys = self.viewing_keys.read().await;

        let mut tree = self.tree.write().await;
        let mut witnesses = self.witnesses.write().await;
        let mut positions = self.nullifier_positions.write().await;
        let mut found_notes = Vec::new();
        // Every (nullifier, spending_tx) seen in this batch. Spent-marking is
        // deferred until after the discovered notes are persisted, so a note
        // both discovered AND spent within the same batch/run still gets marked
        // — check_spent_nullifier is a DB UPDATE that cannot match a note still
        // only in memory (gap A′: a full rescan discovers and spends in one run).
        let mut batch_spends: Vec<([u8; 32], String)> = Vec::new();

        tracing::debug!(
            "[WitnessSync] Processing blocks {}-{}, {} known positions, {} existing witnesses",
            first_height,
            last_height,
            known_positions.len(),
            witnesses.len()
        );

        for block in blocks {
            for tx in &block.transactions {
                for action in &tx.orchard_actions {
                    let current_pos = tree.position();

                    // 1. Update all existing witnesses with this commitment
                    for witness in witnesses.values_mut() {
                        let hash = Self::parse_commitment(&action.cmx)?;
                        witness.append(hash)
                            .map_err(|_| OrchardError::Scanner("Failed to update witness".to_string()))?;
                    }

                    // 2. Check if this position belongs to a known note
                    let is_known_position = known_positions.contains_key(&current_pos);

                    // 3. Try to decrypt (find new notes)
                    let mut found_note = None;
                    for vk in viewing_keys.values() {
                        if let Some(note) = self.try_decrypt_note(vk, action, &tx.hash, block.height) {
                            found_note = Some(note);
                            break;
                        }
                    }

                    // 4. Add commitment to tree
                    tree.append_commitment(&action.cmx)?;

                    // 5. Create witness for notes at this position (only if not already tracked)
                    if is_known_position {
                        // Existing note from DB - only create witness if not already loaded
                        let nullifier = known_positions.get(&current_pos).unwrap().clone();
                        if !witnesses.contains_key(&nullifier) {
                            // First time seeing this note (no witness_state in DB)
                            // Create witness from current tree state
                            if let Some(new_witness) = tree.create_witness_from_current() {
                                tracing::info!(
                                    "[WitnessSync] Creating new witness for note at position {}",
                                    current_pos
                                );
                                witnesses.insert(nullifier.clone(), new_witness);
                                positions.insert(nullifier, current_pos);
                            }
                        }
                        // If witness already exists (loaded from DB), it was updated in step 1
                    }

                    if let Some(mut note) = found_note {
                        // New note discovered - always create witness
                        note.position = current_pos;
                        let nullifier_hex = hex::encode(&note.nullifier);
                        if let Some(new_witness) = tree.create_witness_from_current() {
                            witnesses.insert(nullifier_hex.clone(), new_witness);
                            positions.insert(nullifier_hex, current_pos);
                        }
                        found_notes.push(note);
                    }

                    // 5. Record this spend; it is applied after the discovered
                    // notes are persisted (see batch_spends / gap A′).
                    batch_spends.push((action.nullifier, tx.hash.clone()));
                }
            }

            tree.set_block_height(block.height);
        }

        let witness_count = witnesses.len();
        // Release tree/witness locks before any DB I/O.
        drop(positions);
        drop(witnesses);
        drop(tree);
        drop(viewing_keys);

        // Persist discovered notes first, then mark every spend seen in this
        // batch. Order matters: a note discovered AND spent in the same batch is
        // already in the DB when we mark it, so its is_spent flag is set (A′).
        if !found_notes.is_empty() {
            self.save_notes(&found_notes).await?;
        }
        for (nullifier, tx_hash) in &batch_spends {
            self.check_spent_nullifier(nullifier, tx_hash, last_height).await;
        }

        tracing::info!(
            "[WitnessSync] Processed blocks {}-{}: {} new notes, {} witnesses tracked",
            first_height,
            last_height,
            found_notes.len(),
            witness_count
        );

        Ok(found_notes)
    }

    /// Parse commitment bytes to MerkleHashOrchard
    fn parse_commitment(cmx: &[u8; 32]) -> OrchardResult<MerkleHashOrchard> {
        use subtle::CtOption;
        let hash_opt: CtOption<MerkleHashOrchard> = MerkleHashOrchard::from_bytes(cmx);
        if hash_opt.is_some().into() {
            Ok(hash_opt.unwrap())
        } else {
            Err(OrchardError::Scanner(format!(
                "Invalid commitment: {}",
                hex::encode(cmx)
            )))
        }
    }

    /// Try to decrypt a note (simplified version)
    fn try_decrypt_note(
        &self,
        viewing_key: &OrchardViewingKey,
        action: &CompactOrchardAction,
        tx_hash: &str,
        block_height: u64,
    ) -> Option<OrchardNote> {
        use orchard::keys::PreparedIncomingViewingKey;
        use orchard::note::{ExtractedNoteCommitment, Nullifier};
        use orchard::note_encryption::{CompactAction, OrchardDomain};
        use zcash_note_encryption::{batch, EphemeralKeyBytes, COMPACT_NOTE_SIZE};
        use subtle::CtOption;

        if action.ciphertext.len() < COMPACT_NOTE_SIZE {
            return None;
        }

        let nullifier = {
            let nf_option: CtOption<Nullifier> = Nullifier::from_bytes(&action.nullifier);
            if bool::from(nf_option.is_some()) {
                nf_option.unwrap()
            } else {
                return None;
            }
        };

        let cmx = {
            let cmx_option: CtOption<ExtractedNoteCommitment> = ExtractedNoteCommitment::from_bytes(&action.cmx);
            if bool::from(cmx_option.is_some()) {
                cmx_option.unwrap()
            } else {
                return None;
            }
        };

        let enc_ciphertext: [u8; COMPACT_NOTE_SIZE] = action.ciphertext[..COMPACT_NOTE_SIZE]
            .try_into()
            .ok()?;

        let fvk = viewing_key.fvk();

        for scope in [orchard::keys::Scope::External, orchard::keys::Scope::Internal] {
            let ivk = fvk.to_ivk(scope);
            let prepared_ivk = PreparedIncomingViewingKey::new(&ivk);

            let compact_action = CompactAction::from_parts(
                nullifier,
                cmx,
                EphemeralKeyBytes(action.ephemeral_key),
                enc_ciphertext,
            );
            let domain = OrchardDomain::for_compact_action(&compact_action);

            let results = batch::try_compact_note_decryption(
                &[prepared_ivk],
                &[(domain, compact_action)],
            );

            if let Some(Some(((note, recipient), _))) = results.into_iter().next() {
                let value_zatoshis = note.value().inner();
                let note_nullifier = note.nullifier(fvk);
                let recipient_bytes = recipient.to_raw_address_bytes();
                let rho_bytes = note.rho().to_bytes();
                let rseed_bytes = *note.rseed().as_bytes();

                return Some(OrchardNote {
                    id: None,
                    wallet_id: viewing_key.wallet_id,
                    account_id: viewing_key.account_index,
                    tx_hash: tx_hash.to_string(),
                    block_height,
                    note_commitment: action.cmx,
                    nullifier: note_nullifier.to_bytes(),
                    value_zatoshis,
                    position: 0,  // Will be set by caller
                    is_spent: false,
                    memo: None,
                    merkle_path: None,
                    recipient: recipient_bytes,
                    rho: rho_bytes,
                    rseed: rseed_bytes,
                    witness_data: None,
                });
            }
        }

        None
    }

    /// Check if a nullifier corresponds to a spent note
    async fn check_spent_nullifier(&self, nullifier: &[u8; 32], tx_hash: &str, block_height: u64) {
        let nullifier_hex = hex::encode(nullifier);

        // Try to mark as spent in database
        if let Err(e) = self.db_repo.mark_note_spent(&nullifier_hex, tx_hash).await {
            // This is fine - most nullifiers won't be ours
            tracing::trace!(
                "[WitnessSync] Nullifier check at height {}: {}",
                block_height,
                e
            );
        }
    }

    /// Save current state to database
    ///
    /// Only saves tree_state and witness_state (IncrementalWitness).
    /// auth_path and root are computed in real-time during transfers.
    pub async fn save_state(&self) -> OrchardResult<()> {
        let tree = self.tree.read().await;
        let witnesses = self.witnesses.read().await;

        // Save tree state
        let tree_data = tree.serialize_tree()?;
        let tree_height = tree.block_height();
        let tree_size = tree.position();

        self.db_repo.save_tree_state(&tree_data, tree_height, tree_size).await
            .map_err(|e| OrchardError::DatabaseError(e.to_string()))?;

        // Save witness states only (auth_path/root computed at transfer time)
        let mut saved_count = 0;
        for (nullifier, witness) in witnesses.iter() {
            if let Ok(witness_data) = OrchardTreeTracker::serialize_witness(witness) {
                if self.db_repo.save_witness_state(nullifier, &witness_data).await
                    .map_err(|e| OrchardError::DatabaseError(e.to_string()))?
                {
                    saved_count += 1;
                }
            }
        }

        tracing::info!(
            "[WitnessSync] Saved state: tree_height={}, tree_size={}, witnesses={}",
            tree_height,
            tree_size,
            saved_count
        );

        Ok(())
    }

    /// Get witness data for spending a note
    ///
    /// This returns the current witness data. If the tree is behind chain tip,
    /// it will update the witness first.
    pub async fn get_witness_for_spending(&self, nullifier: &str) -> OrchardResult<Option<WitnessData>> {
        let witnesses = self.witnesses.read().await;
        let positions = self.nullifier_positions.read().await;

        if let Some(witness) = witnesses.get(nullifier) {
            if let Some(path) = witness.path() {
                let position = positions.get(nullifier).copied()
                    .unwrap_or_else(|| path.position().into());

                return Ok(Some(WitnessData {
                    position,
                    auth_path: path.path_elems().iter().map(|h| h.to_bytes()).collect(),
                    root: witness.root().to_bytes(),
                }));
            }
        }

        Ok(None)
    }

    /// Get the Orchard MerklePath directly for a note (using proper conversion)
    ///
    /// This uses `OrchardMerklePath::from()` which is the correct way to convert
    /// from incrementalmerkletree::MerklePath to orchard::tree::MerklePath.
    pub async fn get_orchard_merkle_path(&self, nullifier: &str) -> Option<orchard::tree::MerklePath> {
        let witnesses = self.witnesses.read().await;

        if let Some(witness) = witnesses.get(nullifier) {
            if let Some(path) = witness.path() {
                // Use the proper From conversion
                return Some(orchard::tree::MerklePath::from(path));
            }
        }

        None
    }

    /// Get current tree anchor as orchard::tree::Anchor
    pub async fn get_orchard_anchor(&self) -> orchard::tree::Anchor {
        let tree = self.tree.read().await;
        tree.get_anchor()
    }

    /// Get current tree height
    pub async fn get_tree_height(&self) -> u64 {
        let tree = self.tree.read().await;
        tree.block_height()
    }

    /// Get current tree position (number of commitments)
    pub async fn get_tree_position(&self) -> u64 {
        let tree = self.tree.read().await;
        tree.position()
    }

    /// Get tree tracker reference (for direct access)
    pub fn tree(&self) -> &Arc<RwLock<OrchardTreeTracker>> {
        &self.tree
    }

    /// Build known positions map from database notes
    pub async fn build_known_positions_map(&self) -> OrchardResult<HashMap<u64, String>> {
        let wallet_ids = self.get_wallet_ids().await;
        let note_infos = self.db_repo.load_witness_states(&wallet_ids).await
            .map_err(|e| OrchardError::DatabaseError(e.to_string()))?;

        let mut map = HashMap::new();
        for info in note_infos {
            if let Some(pos) = info.witness_position {
                map.insert(pos, info.nullifier);
            }
        }

        Ok(map)
    }

    /// Get wallet balance from database
    pub async fn get_wallet_balance(&self, wallet_id: i32) -> super::scanner::ShieldedBalance {
        use super::scanner::ShieldedBalance;
        use super::ShieldedPool;

        match self.db_repo.get_balance(wallet_id).await {
            Ok(balance) => {
                let notes_count = self.db_repo.get_notes_count(wallet_id).await.unwrap_or(0);
                ShieldedBalance::new(
                    ShieldedPool::Orchard,
                    balance,
                    balance,  // All unspent are spendable
                    notes_count as u32,
                )
            }
            Err(e) => {
                tracing::warn!("[WitnessSync] Failed to get balance: {}", e);
                ShieldedBalance::new(ShieldedPool::Orchard, 0, 0, 0)
            }
        }
    }

    /// Get spendable notes with witnesses for a wallet
    ///
    /// This implements the design from orchard_witness_sync_design.md section 3.3:
    /// 1. Load note's witness_state from DB
    /// 2. Deserialize to IncrementalWitness
    /// 3. Compute auth_path and root from the witness in real-time
    pub async fn get_spendable_notes_with_witnesses(&self, wallet_id: i32) -> Vec<OrchardNote> {
        // Load notes with spending data
        let db_notes = match self.db_repo.get_spendable_notes(wallet_id).await {
            Ok(notes) => notes,
            Err(e) => {
                tracing::warn!("[WitnessSync] Failed to load notes: {}", e);
                return Vec::new();
            }
        };

        // Load witness states from memory (loaded during initialize())
        let witnesses = self.witnesses.read().await;
        let positions = self.nullifier_positions.read().await;

        let mut result = Vec::new();

        for db_note in db_notes {
            // Parse spending data (recipient, rho, rseed)
            let (recipient, rho, rseed) = match (&db_note.recipient, &db_note.rho, &db_note.rseed) {
                (Some(r), Some(rh), Some(rs)) => {
                    let recipient = match hex::decode(r) {
                        Ok(bytes) if bytes.len() == 43 => {
                            let mut arr = [0u8; 43];
                            arr.copy_from_slice(&bytes);
                            arr
                        }
                        _ => continue,
                    };
                    let rho = match hex::decode(rh) {
                        Ok(bytes) if bytes.len() == 32 => {
                            let mut arr = [0u8; 32];
                            arr.copy_from_slice(&bytes);
                            arr
                        }
                        _ => continue,
                    };
                    let rseed = match hex::decode(rs) {
                        Ok(bytes) if bytes.len() == 32 => {
                            let mut arr = [0u8; 32];
                            arr.copy_from_slice(&bytes);
                            arr
                        }
                        _ => continue,
                    };
                    (recipient, rho, rseed)
                }
                _ => continue,
            };

            // Parse nullifier
            let nullifier: [u8; 32] = match hex::decode(&db_note.nullifier) {
                Ok(bytes) if bytes.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    arr
                }
                _ => continue,
            };

            // Get witness from memory and compute auth_path/root in real-time
            let witness_data = if let Some(witness) = witnesses.get(&db_note.nullifier) {
                // Compute auth_path and root from IncrementalWitness
                if let Some(path) = witness.path() {
                    let position = positions.get(&db_note.nullifier).copied()
                        .unwrap_or_else(|| path.position().into());

                    let auth_path: Vec<[u8; 32]> = path.path_elems()
                        .iter()
                        .map(|h| h.to_bytes())
                        .collect();

                    let root = witness.root().to_bytes();

                    tracing::debug!(
                        "[WitnessSync] Computed witness for note {}: position={}, root={}",
                        &db_note.nullifier[..16],
                        position,
                        hex::encode(&root[..16])
                    );

                    Some(WitnessData { position, auth_path, root })
                } else {
                    tracing::warn!(
                        "[WitnessSync] Witness has no path for note {}",
                        &db_note.nullifier[..16]
                    );
                    None
                }
            } else {
                tracing::debug!(
                    "[WitnessSync] No witness in memory for note {}, skipping",
                    &db_note.nullifier[..16]
                );
                None
            };

            // Skip notes without valid witness
            let witness_data = match witness_data {
                Some(wd) if wd.auth_path.len() == 32 => wd,
                _ => continue,
            };

            result.push(OrchardNote {
                id: Some(db_note.id as i64),
                wallet_id: Some(wallet_id),
                account_id: 0,
                tx_hash: db_note.tx_hash.clone(),
                block_height: db_note.block_height,
                note_commitment: [0u8; 32],
                nullifier,
                value_zatoshis: db_note.value_zatoshis,
                position: witness_data.position,
                is_spent: db_note.is_spent,
                memo: db_note.memo.clone(),
                merkle_path: None,
                recipient,
                rho,
                rseed,
                witness_data: Some(witness_data),
            });
        }

        tracing::info!(
            "[WitnessSync] Returning {} spendable notes with computed witnesses for wallet {}",
            result.len(),
            wallet_id
        );

        result
    }

    /// Get scan progress
    pub async fn get_progress(&self) -> super::scanner::ScanProgress {
        let tree = self.tree.read().await;
        let chain_tip = self.get_chain_height().await.unwrap_or(0);
        let tree_height = tree.block_height();
        let activation = self.orchard_activation_height().await;

        let progress_pct = if chain_tip > 0 && chain_tip > activation {
            let total = chain_tip - activation;
            let scanned = tree_height.saturating_sub(activation);
            (scanned as f64 / total as f64) * 100.0
        } else {
            0.0
        };

        // Count notes from all wallets
        let wallet_ids = self.get_wallet_ids().await;
        let mut total_notes = 0u64;
        for wallet_id in &wallet_ids {
            total_notes += self.db_repo.get_notes_count(*wallet_id).await.unwrap_or(0) as u64;
        }

        super::scanner::ScanProgress {
            chain: "zcash".to_string(),
            scan_type: "orchard".to_string(),
            last_scanned_height: tree_height,
            chain_tip_height: chain_tip,
            progress_percent: progress_pct.min(100.0),
            estimated_seconds_remaining: None,
            is_scanning: false,
            notes_found: total_notes,
        }
    }

    /// Refresh witnesses for a wallet before spending
    ///
    /// This ensures witnesses are up to date with the latest chain state.
    /// Returns true if witnesses were refreshed.
    pub async fn refresh_witnesses_for_spending(&self, _wallet_id: i32) -> OrchardResult<bool> {
        let chain_tip = self.get_chain_height().await?;
        let tree_height = self.get_tree_height().await;

        // If tree is already at chain tip, no need to refresh
        if tree_height >= chain_tip {
            tracing::debug!("[WitnessSync] Tree already at chain tip {}", chain_tip);
            return Ok(false);
        }

        tracing::info!(
            "[WitnessSync] Refreshing witnesses: tree={} -> chain_tip={}",
            tree_height,
            chain_tip
        );

        // Full incremental scan — NOT a commitment-only append.
        //
        // Advancing the tree over these blocks MUST also run note discovery and
        // nullifier spent-marking. `process_blocks` does all three (append to
        // tree + update/create witnesses + discover notes + mark spends), then
        // sets the tree height. If we only appended commitments here (the old
        // behaviour) the tree height would jump to chain_tip while the notes /
        // spends in those blocks were never processed — and the sync path,
        // gated on `tree_height < chain_tip`, would then treat them as already
        // scanned and skip them forever. That silently dropped received notes,
        // change notes, and is_spent flags for any tx mined between two spends
        // (Bug A: 5c15@241 zero-recognized after a pre-spend refresh crossed it).
        let known_positions = self.build_known_positions_map().await?;
        let mut current = tree_height + 1;
        let batch_size = 100u64;
        while current <= chain_tip {
            let end = std::cmp::min(current + batch_size - 1, chain_tip);
            let blocks = self.fetch_blocks(current, end).await?;
            if !blocks.is_empty() {
                // process_blocks persists discovered notes and marks spends.
                let _ = self.process_blocks(blocks, &known_positions).await?;
            }
            current = end + 1;
        }

        // Persist tree + witness state after the scan
        self.save_state().await?;

        Ok(true)
    }

    /// Fetch commitments from a range of blocks (commitment-only helper; the
    /// spend refresh now does a full scan via process_blocks instead).
    #[allow(dead_code)]
    async fn fetch_commitments_range(&self, from_height: u64, to_height: u64) -> OrchardResult<Vec<[u8; 32]>> {
        let mut commitments = Vec::new();

        // Fetch in batches
        let batch_size = 100u64;
        let mut current = from_height;

        while current <= to_height {
            let end = std::cmp::min(current + batch_size - 1, to_height);
            let blocks = self.fetch_blocks(current, end).await?;

            for block in blocks {
                for tx in block.transactions {
                    for action in tx.orchard_actions {
                        commitments.push(action.cmx);
                    }
                }
            }

            current = end + 1;
        }

        Ok(commitments)
    }

    /// Fetch blocks from RPC
    pub async fn fetch_blocks(&self, from_height: u64, to_height: u64) -> OrchardResult<Vec<CompactBlock>> {
        let mut blocks = Vec::new();

        for height in from_height..=to_height {
            match self.fetch_block(height).await {
                Ok(block) => blocks.push(block),
                Err(e) => {
                    tracing::warn!("[WitnessSync] Failed to fetch block {}: {}", height, e);
                }
            }
        }

        Ok(blocks)
    }

    /// Fetch a single block
    async fn fetch_block(&self, height: u64) -> OrchardResult<CompactBlock> {
        // Get block hash
        let hash_request = serde_json::json!({
            "jsonrpc": "1.0",
            "id": "witness_sync",
            "method": "getblockhash",
            "params": [height]
        });

        let response = self.rpc_client
            .post(&self.rpc_url)
            .basic_auth(&self.rpc_user, Some(&self.rpc_password))
            .json(&hash_request)
            .send()
            .await
            .map_err(|e| OrchardError::RpcError(e.to_string()))?;

        let result: serde_json::Value = response.json().await
            .map_err(|e| OrchardError::RpcError(e.to_string()))?;

        let hash = result["result"].as_str()
            .ok_or_else(|| OrchardError::RpcError("Missing block hash".to_string()))?;

        // Get block with verbosity 2
        let block_request = serde_json::json!({
            "jsonrpc": "1.0",
            "id": "witness_sync",
            "method": "getblock",
            "params": [hash, 2]
        });

        let response = self.rpc_client
            .post(&self.rpc_url)
            .basic_auth(&self.rpc_user, Some(&self.rpc_password))
            .json(&block_request)
            .send()
            .await
            .map_err(|e| OrchardError::RpcError(e.to_string()))?;

        let result: serde_json::Value = response.json().await
            .map_err(|e| OrchardError::RpcError(e.to_string()))?;

        self.parse_block(&result["result"])
    }

    /// Parse block JSON to CompactBlock
    fn parse_block(&self, block: &serde_json::Value) -> OrchardResult<CompactBlock> {
        let height = block["height"].as_u64()
            .ok_or_else(|| OrchardError::RpcError("Missing height".to_string()))?;

        let hash_hex = block["hash"].as_str().unwrap_or("");
        let mut hash = [0u8; 32];
        if let Ok(bytes) = hex::decode(hash_hex) {
            if bytes.len() == 32 {
                hash.copy_from_slice(&bytes);
            }
        }

        let mut transactions = Vec::new();

        if let Some(txs) = block["tx"].as_array() {
            for tx in txs {
                if let Some(orchard) = tx["orchard"].as_object() {
                    if let Some(actions) = orchard.get("actions").and_then(|a| a.as_array()) {
                        let mut orchard_actions = Vec::new();

                        for action in actions {
                            let cmx = self.parse_hex_32(action["cmx"].as_str().unwrap_or(""))?;
                            let nullifier = self.parse_hex_32(action["nullifier"].as_str().unwrap_or(""))?;
                            let ephemeral_key = self.parse_hex_32(action["ephemeralKey"].as_str().unwrap_or(""))?;
                            let ciphertext = hex::decode(action["encCiphertext"].as_str().unwrap_or(""))
                                .unwrap_or_default();

                            orchard_actions.push(CompactOrchardAction {
                                cmx,
                                nullifier,
                                ephemeral_key,
                                ciphertext,
                            });
                        }

                        if !orchard_actions.is_empty() {
                            transactions.push(super::scanner::CompactTransaction {
                                hash: tx["txid"].as_str().unwrap_or("").to_string(),
                                orchard_actions,
                            });
                        }
                    }
                }
            }
        }

        Ok(CompactBlock {
            height,
            hash,
            transactions,
        })
    }

    /// Parse hex string to 32-byte array
    fn parse_hex_32(&self, hex_str: &str) -> OrchardResult<[u8; 32]> {
        let bytes = hex::decode(hex_str)
            .map_err(|e| OrchardError::RpcError(format!("Invalid hex: {}", e)))?;

        if bytes.len() != 32 {
            return Err(OrchardError::RpcError(format!(
                "Expected 32 bytes, got {}",
                bytes.len()
            )));
        }

        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(arr)
    }

    /// Check if anchor is too old for spending
    pub async fn is_anchor_too_old(&self) -> bool {
        const MAX_ANCHOR_AGE_BLOCKS: u64 = 100;

        let tree_height = self.get_tree_height().await;
        if let Ok(chain_tip) = self.get_chain_height().await {
            let age = chain_tip.saturating_sub(tree_height);
            age > MAX_ANCHOR_AGE_BLOCKS
        } else {
            true
        }
    }

    /// Save discovered notes to database
    pub async fn save_notes(&self, notes: &[OrchardNote]) -> OrchardResult<()> {
        for note in notes {
            // Skip notes without valid wallet_id
            let wallet_id = match note.wallet_id {
                Some(id) if id > 0 => id,
                _ => {
                    tracing::warn!(
                        "[WitnessSync] Skipping note without valid wallet_id: {}",
                        hex::encode(&note.nullifier)
                    );
                    continue;
                }
            };

            let nullifier_hex = hex::encode(&note.nullifier);
            let recipient_hex = hex::encode(&note.recipient);
            let rho_hex = hex::encode(&note.rho);
            let rseed_hex = hex::encode(&note.rseed);

            self.db_repo.save_note_full(
                wallet_id,
                &nullifier_hex,
                note.value_zatoshis,
                note.block_height,
                &note.tx_hash,
                note.position as u32,
                note.memo.as_deref(),
                &recipient_hex,
                &rho_hex,
                &rseed_hex,
                note.position,
            ).await.map_err(|e| OrchardError::DatabaseError(e.to_string()))?;
        }

        Ok(())
    }

    /// Get minimum scan height from database
    pub async fn get_min_scan_height(&self) -> OrchardResult<u64> {
        let wallet_ids = self.get_wallet_ids().await;
        let mut min_height = u64::MAX;

        for wallet_id in &wallet_ids {
            if let Ok(Some(state)) = self.db_repo.get_sync_state(*wallet_id).await {
                min_height = min_height.min(state.last_scanned_height);
            }
        }

        if min_height == u64::MAX {
            min_height = self.orchard_activation_height().await; // network Orchard activation
        }

        Ok(min_height)
    }

    /// Check if there are notes without witness_state that need rescanning
    /// Returns the minimum block_height if rescanning is needed, None otherwise
    pub async fn check_notes_need_rescan(&self) -> OrchardResult<Option<u64>> {
        let wallet_ids = self.get_wallet_ids().await;
        let min_height = self.db_repo.get_min_height_notes_without_witness_state(&wallet_ids).await
            .map_err(|e| OrchardError::DatabaseError(e.to_string()))?;

        if let Some(height) = min_height {
            tracing::warn!(
                "[WitnessSync] Found notes without witness_state, earliest at block {}. Need to rescan.",
                height
            );
        }

        Ok(min_height)
    }

    /// Reset tree state and prepare for rescanning from a given height
    /// This is needed when notes exist without witness_state
    pub async fn reset_for_rescan(&self, from_height: u64) -> OrchardResult<()> {
        // Clear existing witnesses since they'll be rebuilt
        {
            let mut witnesses = self.witnesses.write().await;
            let mut positions = self.nullifier_positions.write().await;
            witnesses.clear();
            positions.clear();
        }

        // Delete saved tree state from DB
        self.db_repo.delete_tree_state().await
            .map_err(|e| OrchardError::DatabaseError(e.to_string()))?;

        // Initialize from frontier at height-1, floored at network activation
        let activation = self.orchard_activation_height().await;
        let frontier_height = from_height.saturating_sub(1).max(activation);
        tracing::info!(
            "[WitnessSync] Resetting tree state. Will init from frontier at height {}",
            frontier_height
        );

        self.init_from_frontier(frontier_height).await?;

        Ok(())
    }

    /// Update sync state for a wallet
    pub async fn update_sync_state(&self, wallet_id: i32, height: u64) -> OrchardResult<()> {
        let notes_count = self.db_repo.get_notes_count(wallet_id).await.unwrap_or(0);
        self.db_repo.upsert_sync_state(wallet_id, height, notes_count as u32).await
            .map_err(|e| OrchardError::DatabaseError(e.to_string()))
    }
}
