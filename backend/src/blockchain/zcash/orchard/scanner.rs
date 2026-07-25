//! Block scanner for Orchard notes
//!
//! Scans the blockchain to find notes that belong to a viewing key
//! and maintains the commitment tree for spending.

#![allow(dead_code)]

use super::{
    constants::MIN_CONFIRMATIONS,
    keys::OrchardViewingKey,
    tree::{OrchardTreeTracker, WitnessData},
    OrchardResult, ShieldedPool,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Orchard and cryptography imports for note decryption
use subtle::CtOption;

/// Progress information for blockchain scanning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgress {
    /// Chain being scanned
    pub chain: String,

    /// Type of scan (orchard, sapling, etc.)
    pub scan_type: String,

    /// Last fully scanned block height
    pub last_scanned_height: u64,

    /// Current chain tip height
    pub chain_tip_height: u64,

    /// Percentage complete (0-100)
    pub progress_percent: f64,

    /// Estimated time remaining in seconds
    pub estimated_seconds_remaining: Option<u64>,

    /// Whether scanning is currently active
    pub is_scanning: bool,

    /// Number of notes found
    pub notes_found: u64,
}

impl ScanProgress {
    /// Create a new scan progress tracker
    pub fn new(chain: &str, scan_type: &str, birthday_height: u64, chain_tip: u64) -> Self {
        Self {
            chain: chain.to_string(),
            scan_type: scan_type.to_string(),
            last_scanned_height: birthday_height,
            chain_tip_height: chain_tip,
            progress_percent: 0.0,
            estimated_seconds_remaining: None,
            is_scanning: false,
            notes_found: 0,
        }
    }

    /// Update progress after scanning a range
    pub fn update(&mut self, scanned_to: u64, notes_found: u64, elapsed_secs: f64) {
        self.last_scanned_height = scanned_to;
        self.notes_found += notes_found;

        let total_blocks = self.chain_tip_height - self.last_scanned_height;
        let scanned_blocks = scanned_to - self.last_scanned_height;

        if total_blocks > 0 {
            self.progress_percent = (scanned_blocks as f64 / total_blocks as f64) * 100.0;

            if scanned_blocks > 0 && elapsed_secs > 0.0 {
                let blocks_per_sec = scanned_blocks as f64 / elapsed_secs;
                let remaining_blocks = self.chain_tip_height - scanned_to;
                self.estimated_seconds_remaining = Some((remaining_blocks as f64 / blocks_per_sec) as u64);
            }
        } else {
            self.progress_percent = 100.0;
            self.estimated_seconds_remaining = Some(0);
        }
    }

    /// Mark scan as complete
    pub fn complete(&mut self) {
        self.progress_percent = 100.0;
        self.estimated_seconds_remaining = Some(0);
        self.is_scanning = false;
    }
}

/// An Orchard note (received shielded funds)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchardNote {
    /// Unique note ID (internal)
    pub id: Option<i64>,

    /// Wallet ID this note belongs to (for multi-wallet support)
    pub wallet_id: Option<i32>,

    /// Account ID this note belongs to (legacy, kept for compatibility)
    pub account_id: u32,

    /// Transaction hash where this note was received
    pub tx_hash: String,

    /// Block height where confirmed
    pub block_height: u64,

    /// Note commitment
    #[serde(with = "hex_array")]
    pub note_commitment: [u8; 32],

    /// Nullifier (used to mark as spent)
    #[serde(with = "hex_array")]
    pub nullifier: [u8; 32],

    /// Value in zatoshis
    pub value_zatoshis: u64,

    /// Position in the commitment tree
    pub position: u64,

    /// Whether this note has been spent
    pub is_spent: bool,

    /// Decrypted memo (if any)
    pub memo: Option<String>,

    /// Merkle path for spending (populated when needed)
    #[serde(skip)]
    pub merkle_path: Option<Vec<[u8; 32]>>,

    // === Fields required for spending (shielded-to-shielded transfers) ===

    /// Recipient address bytes (43 bytes for Orchard address)
    /// Required to reconstruct the note for spending
    #[serde(with = "hex_array_43", default = "default_recipient")]
    pub recipient: [u8; 43],

    /// Rho (note randomness, 32 bytes)
    /// Required to reconstruct the note for spending
    #[serde(with = "hex_array", default = "default_32")]
    pub rho: [u8; 32],

    /// Random seed bytes (32 bytes)
    /// Required to reconstruct the note for spending
    #[serde(with = "hex_array", default = "default_32")]
    pub rseed: [u8; 32],

    /// Witness data for spending (computed from commitment tree)
    /// Contains the authentication path to prove the note exists
    #[serde(skip)]
    pub witness_data: Option<WitnessData>,

    /// Which shielded pool this note lives in.
    ///
    /// Decides the note plaintext version (Orchard = V2 / Ironwood = V3), which
    /// commitment tree holds its witness, and which anchor a spend must use. A
    /// spend may never mix notes from different pools. Defaults to `Orchard` so
    /// notes persisted before the dual-pool scanner keep their meaning.
    #[serde(default)]
    pub pool: ShieldedPool,
}

fn default_recipient() -> [u8; 43] {
    [0u8; 43]
}

fn default_32() -> [u8; 32] {
    [0u8; 32]
}

/// Hex serialization for fixed-size arrays (32 bytes)
mod hex_array {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom("Invalid length"));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(arr)
    }
}

/// Hex serialization for 43-byte arrays (Orchard address)
mod hex_array_43 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 43], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 43], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 43 {
            return Err(serde::de::Error::custom(format!("Invalid length: expected 43, got {}", bytes.len())));
        }
        let mut arr = [0u8; 43];
        arr.copy_from_slice(&bytes);
        Ok(arr)
    }
}

/// Information about a spent note detected during scanning
#[derive(Debug, Clone)]
pub struct SpentNoteInfo {
    /// The nullifier of the spent note
    pub nullifier: [u8; 32],
    /// The transaction hash where the note was spent
    pub spent_in_tx: String,
    /// The block height where the spend was detected
    pub block_height: u64,
}

/// Scanner for detecting Orchard notes in blocks
pub struct OrchardScanner {
    /// Viewing keys to scan for
    viewing_keys: Vec<OrchardViewingKey>,

    /// Commitment tree tracker for Orchard pool (using incrementalmerkletree)
    tree_tracker: OrchardTreeTracker,

    /// Scanned notes by wallet_id (each wallet has its own note collection)
    notes: HashMap<i32, Vec<OrchardNote>>,

    /// Known nullifiers (spent notes)
    spent_nullifiers: Vec<[u8; 32]>,

    /// Newly detected spent notes (cleared after retrieval)
    newly_spent_notes: Vec<SpentNoteInfo>,

    /// Scan progress
    progress: ScanProgress,
}

impl OrchardScanner {
    /// Create a new scanner with the given viewing keys
    pub fn new(viewing_keys: Vec<OrchardViewingKey>) -> Self {
        // Get the minimum birthday height from all viewing keys
        // This ensures we start scanning from the earliest possible block where notes could exist
        let birthday_height = viewing_keys
            .iter()
            .map(|vk| vk.birthday_height)
            .min()
            .unwrap_or(2_000_000); // Default to Orchard activation height if no keys

        tracing::debug!(
            "[OrchardScanner] Created with {} viewing keys, birthday_height={}",
            viewing_keys.len(),
            birthday_height
        );

        Self {
            viewing_keys,
            tree_tracker: OrchardTreeTracker::new(),
            notes: HashMap::new(),
            spent_nullifiers: Vec::new(),
            newly_spent_notes: Vec::new(),
            progress: ScanProgress::new("zcash", "orchard", birthday_height, 0),
        }
    }

    /// Initialize the tree tracker from a frontier
    ///
    /// This allows resuming from a known tree state without scanning from genesis.
    /// The frontier should be obtained from z_gettreestate RPC.
    pub fn init_from_frontier(
        &mut self,
        frontier_hex: &str,
        position: u64,
        block_height: u64,
    ) -> Result<(), super::tree::TreeError> {
        self.tree_tracker.reset_from_frontier(frontier_hex, position, block_height)?;
        self.progress.last_scanned_height = block_height;
        tracing::info!(
            "[OrchardScanner] Initialized from frontier at height {}, position {}",
            block_height,
            position
        );
        Ok(())
    }

    /// Get current scan progress
    pub fn progress(&self) -> &ScanProgress {
        &self.progress
    }

    /// Get all unspent notes for a wallet
    pub fn get_unspent_notes(&self, wallet_id: i32) -> Vec<OrchardNote> {
        self.notes
            .get(&wallet_id)
            .map(|notes| {
                notes
                    .iter()
                    .filter(|n| !n.is_spent)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get spendable notes (confirmed and not spent) for a wallet
    ///
    /// IMPORTANT: Always fetches fresh witness data from tree_tracker.
    /// The tree_tracker maintains up-to-date witnesses as new commitments are appended.
    /// The witness root must match the current tree state for valid transactions.
    pub fn get_spendable_notes(&self, wallet_id: i32, current_height: u64) -> Vec<OrchardNote> {
        self.notes
            .get(&wallet_id)
            .map(|notes| {
                notes
                    .iter()
                    .filter(|n| {
                        !n.is_spent
                            && current_height >= n.block_height + MIN_CONFIRMATIONS as u64
                    })
                    .cloned()
                    .map(|mut note| {
                        // ALWAYS get fresh witness from tree tracker
                        // The tree_tracker maintains witnesses that are updated as new
                        // commitments are appended. The witness root must reflect the
                        // current tree state for the Zcash node to accept the anchor.
                        // Old witness data (with stale roots) will cause "unknown anchor" errors.
                        let fresh_witness = self.tree_tracker.get_witness(note.position);
                        if fresh_witness.is_some() {
                            note.witness_data = fresh_witness;
                        }
                        // If tree_tracker doesn't have the witness (e.g., after restart),
                        // keep existing witness_data which may need refresh_witnesses_for_spending
                        note
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get total shielded balance for a wallet
    pub fn get_balance(&self, wallet_id: i32) -> u64 {
        self.notes
            .get(&wallet_id)
            .map(|notes| notes.iter().filter(|n| !n.is_spent).map(|n| n.value_zatoshis).sum())
            .unwrap_or(0)
    }

    /// Get spendable balance (confirmed notes only) for a wallet
    pub fn get_spendable_balance(&self, wallet_id: i32, current_height: u64) -> u64 {
        self.notes
            .get(&wallet_id)
            .map(|notes| {
                notes
                    .iter()
                    .filter(|n| {
                        !n.is_spent
                            && current_height >= n.block_height + MIN_CONFIRMATIONS as u64
                    })
                    .map(|n| n.value_zatoshis)
                    .sum()
            })
            .unwrap_or(0)
    }

    /// Scan a range of blocks
    pub async fn scan_blocks(
        &mut self,
        blocks: Vec<CompactBlock>,
        chain_tip: u64,
    ) -> OrchardResult<Vec<OrchardNote>> {
        let start_time = std::time::Instant::now();
        let mut found_notes = Vec::new();
        let block_count = blocks.len();
        let first_height = blocks.first().map(|b| b.height).unwrap_or(0);
        let last_height = blocks.last().map(|b| b.height).unwrap_or(0);

        self.progress.chain_tip_height = chain_tip;
        self.progress.is_scanning = true;

        let viewing_key_count = self.viewing_keys.len();
        let mut total_actions = 0usize;
        let mut total_txs = 0usize;

        tracing::debug!(
            "[Orchard Scan] Starting scan of {} blocks ({}-{}), {} viewing keys registered",
            block_count,
            first_height,
            last_height,
            viewing_key_count
        );

        for block in blocks {
            let block_tx_count = block.transactions.len();
            let block_action_count: usize = block.transactions.iter().map(|tx| tx.orchard_actions.len()).sum();

            if block_action_count > 0 {
                tracing::trace!(
                    "[Orchard Scan] Block {}: {} txs, {} orchard actions",
                    block.height,
                    block_tx_count,
                    block_action_count
                );
            }

            total_txs += block_tx_count;

            // Process each transaction in the block
            for tx in &block.transactions {
                // Process Orchard actions - we need to track ALL commitments
                // to build the correct Merkle tree, not just our notes
                for action in &tx.orchard_actions {
                    total_actions += 1;

                    // First, try to decrypt with each viewing key to see if this is our note
                    let mut found_our_note = false;
                    let mut our_note: Option<OrchardNote> = None;

                    for vk in &self.viewing_keys {
                        // Try decryption with position placeholder (we'll get real position after)
                        if let Some(note) = self.try_decrypt_note_without_position(
                            vk,
                            action,
                            &tx.hash,
                            block.height,
                        ) {
                            found_our_note = true;
                            our_note = Some(note);
                            break; // Found our note, stop checking other keys
                        }
                    }

                    // Add commitment to tree - if it's our note, mark it for witness tracking
                    let position = if found_our_note {
                        // This is our note - mark it for witness tracking
                        match self.tree_tracker.append_and_mark(&action.cmx) {
                            Ok(pos) => pos,
                            Err(e) => {
                                tracing::error!(
                                    "[Orchard Scan] Failed to add commitment to tree: {}",
                                    e
                                );
                                continue;
                            }
                        }
                    } else {
                        // Not our note, just track the commitment
                        match self.tree_tracker.append_commitment(&action.cmx) {
                            Ok(pos) => pos,
                            Err(e) => {
                                tracing::error!(
                                    "[Orchard Scan] Failed to add commitment to tree: {}",
                                    e
                                );
                                continue;
                            }
                        }
                    };

                    // If we found our note, update its position and store it
                    if let Some(mut note) = our_note {
                        note.position = position;
                        // Get the witness data for this note
                        note.witness_data = self.tree_tracker.get_witness(position);

                        tracing::info!(
                            "[Orchard Scan] 🎉 Found note! block={}, tx={}, value={} zatoshis ({:.8} ZEC), position={}, has_witness={}",
                            block.height,
                            &tx.hash[..16],
                            note.value_zatoshis,
                            note.value_zatoshis as f64 / 100_000_000.0,
                            position,
                            note.witness_data.is_some()
                        );

                        found_notes.push(note.clone());

                        // Store the note by wallet_id (each wallet has isolated notes)
                        if let Some(wallet_id) = note.wallet_id {
                            self.notes
                                .entry(wallet_id)
                                .or_insert_with(Vec::new)
                                .push(note);
                        } else {
                            tracing::warn!(
                                "[Orchard Scan] Note has no wallet_id, skipping storage. tx={}",
                                &tx.hash[..16]
                            );
                        }
                    }

                    // Check for spent notes
                    self.check_spent_nullifier(&action.nullifier, &tx.hash, block.height);
                }
            }

            self.progress.last_scanned_height = block.height;
            self.tree_tracker.set_block_height(block.height);
        }

        let elapsed = start_time.elapsed().as_secs_f64();
        self.progress.update(
            self.progress.last_scanned_height,
            found_notes.len() as u64,
            elapsed,
        );

        if self.progress.last_scanned_height >= chain_tip {
            self.progress.complete();
        }

        tracing::info!(
            "[Orchard Scan] Completed: {} blocks ({}-{}), {} txs, {} actions scanned, {} notes found, tree_size={}, witnesses={}, {:.2}s elapsed",
            block_count,
            first_height,
            last_height,
            total_txs,
            total_actions,
            found_notes.len(),
            self.tree_tracker.position(),
            self.tree_tracker.witness_count(),
            elapsed
        );

        Ok(found_notes)
    }

    /// Try to decrypt a note without assigning position yet
    /// Used to check if a note belongs to us before adding to tree
    fn try_decrypt_note_without_position(
        &self,
        viewing_key: &OrchardViewingKey,
        action: &CompactOrchardAction,
        tx_hash: &str,
        block_height: u64,
    ) -> Option<OrchardNote> {
        // Call the existing try_decrypt_note with a placeholder position
        // The real position will be set after the commitment is added to the tree
        self.try_decrypt_note(viewing_key, action, tx_hash, block_height, 0)
    }

    /// Try to decrypt a note with a viewing key using official Orchard decryption API
    fn try_decrypt_note(
        &self,
        viewing_key: &OrchardViewingKey,
        action: &CompactOrchardAction,
        tx_hash: &str,
        block_height: u64,
        position: u64,
    ) -> Option<OrchardNote> {
        use orchard::keys::PreparedIncomingViewingKey;
        use orchard::note::{ExtractedNoteCommitment, Nullifier};
        use orchard::note_encryption::{CompactAction, OrchardDomain};
        use zcash_note_encryption::{batch, EphemeralKeyBytes, COMPACT_NOTE_SIZE};

        // Ensure ciphertext is long enough for compact decryption (52 bytes)
        if action.ciphertext.len() < COMPACT_NOTE_SIZE {
            tracing::trace!("[Decrypt] Ciphertext too short: {} < {}", action.ciphertext.len(), COMPACT_NOTE_SIZE);
            return None;
        }

        // Parse nullifier
        let nullifier = {
            let nf_option: CtOption<Nullifier> = Nullifier::from_bytes(&action.nullifier);
            if bool::from(nf_option.is_some()) {
                nf_option.unwrap()
            } else {
                tracing::trace!("[Decrypt] Invalid nullifier bytes");
                return None;
            }
        };

        // Parse note commitment (cmx)
        let cmx = {
            let cmx_option: CtOption<ExtractedNoteCommitment> = ExtractedNoteCommitment::from_bytes(&action.cmx);
            if bool::from(cmx_option.is_some()) {
                cmx_option.unwrap()
            } else {
                tracing::trace!("[Decrypt] Invalid cmx bytes");
                return None;
            }
        };

        // Create compact ciphertext array
        let enc_ciphertext: [u8; COMPACT_NOTE_SIZE] = action.ciphertext[..COMPACT_NOTE_SIZE]
            .try_into()
            .ok()?;

        // Get the incoming viewing key from the full viewing key
        let fvk = viewing_key.fvk();

        // Try External scope first, then Internal scope
        for scope in [orchard::keys::Scope::External, orchard::keys::Scope::Internal] {
            let ivk = fvk.to_ivk(scope);
            let prepared_ivk = PreparedIncomingViewingKey::new(&ivk);

            // Recreate CompactAction and domain for each attempt
            let compact_action_for_scope = CompactAction::from_parts(
                nullifier,
                cmx,
                EphemeralKeyBytes(action.ephemeral_key),
                enc_ciphertext,
            );
            let domain_for_scope = OrchardDomain::for_compact_action(&compact_action_for_scope);

            // Use the official batch decryption API
            // batch::try_compact_note_decryption returns Vec<Option<((Note, Address), ivk_index)>>
            let results = batch::try_compact_note_decryption(
                &[prepared_ivk],
                &[(domain_for_scope, compact_action_for_scope)],
            );

            if let Some(Some(((note, recipient), _ivk_idx))) = results.into_iter().next() {
                // Successfully decrypted the note!
                let value_zatoshis = note.value().inner();

                // Compute the nullifier using the full viewing key
                let note_nullifier = note.nullifier(fvk);

                // Extract data needed for spending
                let recipient_bytes = recipient.to_raw_address_bytes();
                let rho_bytes = note.rho().to_bytes();
                let rseed_bytes = *note.rseed().as_bytes();

                tracing::debug!(
                    "[Decrypt] ✅ Successfully decrypted note: value={} zatoshis, scope={:?}, \
                     recipient={}, rho={}, rseed={}",
                    value_zatoshis,
                    scope,
                    hex::encode(&recipient_bytes[..8]),
                    hex::encode(&rho_bytes[..8]),
                    hex::encode(&rseed_bytes[..8])
                );

                return Some(OrchardNote {
                    id: None,
                    wallet_id: viewing_key.wallet_id,
                    account_id: viewing_key.account_index,
                    tx_hash: tx_hash.to_string(),
                    block_height,
                    note_commitment: action.cmx,
                    nullifier: note_nullifier.to_bytes(),
                    value_zatoshis,
                    position,
                    is_spent: false,
                    memo: None, // Compact blocks don't include memo
                    merkle_path: None,
                    // Spending data
                    recipient: recipient_bytes,
                    rho: rho_bytes,
                    rseed: rseed_bytes,
                    // Witness data will be set after adding to tree
                    witness_data: None,
                    // This scanner decrypts with OrchardDomain (V2 plaintexts)
                    // against the Orchard tree, so anything it finds is an old-pool
                    // note. Ironwood notes are discovered by the dual-pool scan in
                    // witness_sync.
                    pool: ShieldedPool::Orchard,
                });
            }
        }

        // Decryption failed for all scopes - this note doesn't belong to this viewing key
        None
    }

    /// Check if a nullifier corresponds to one of our notes
    fn check_spent_nullifier(&mut self, nullifier: &[u8; 32], tx_hash: &str, block_height: u64) {
        // Check all notes
        for notes in self.notes.values_mut() {
            for note in notes.iter_mut() {
                if &note.nullifier == nullifier && !note.is_spent {
                    note.is_spent = true;
                    self.spent_nullifiers.push(*nullifier);

                    // Record this newly spent note for database sync
                    self.newly_spent_notes.push(SpentNoteInfo {
                        nullifier: *nullifier,
                        spent_in_tx: tx_hash.to_string(),
                        block_height,
                    });

                    tracing::info!(
                        "Note spent: {} zatoshis, nullifier: {}, tx: {}",
                        note.value_zatoshis,
                        hex::encode(nullifier),
                        tx_hash
                    );
                }
            }
        }
    }

    /// Get and clear newly detected spent notes
    /// Call this after scan_blocks to get the list of notes that were marked as spent
    pub fn take_newly_spent_notes(&mut self) -> Vec<SpentNoteInfo> {
        std::mem::take(&mut self.newly_spent_notes)
    }

    /// Get the current commitment tree anchor (tree root)
    pub fn get_anchor(&self) -> [u8; 32] {
        self.tree_tracker.root()
    }

    /// Get the current tree root - alias for get_anchor for clarity
    pub fn get_current_root(&self) -> [u8; 32] {
        self.tree_tracker.root()
    }

    /// Get the Orchard anchor for transaction building
    pub fn get_orchard_anchor(&self) -> orchard::tree::Anchor {
        self.tree_tracker.get_anchor()
    }

    /// Get the Merkle path for a note at the given position
    pub fn get_merkle_path(&self, position: u64) -> Option<orchard::tree::MerklePath> {
        self.tree_tracker.get_orchard_merkle_path(position)
    }

    /// Get the tree tracker (for advanced operations)
    pub fn tree_tracker(&self) -> &OrchardTreeTracker {
        &self.tree_tracker
    }

    /// Mark a note as spent by nullifier
    pub fn mark_spent(&mut self, nullifier: &[u8; 32], tx_hash: &str, block_height: u64) {
        self.check_spent_nullifier(nullifier, tx_hash, block_height);
    }

    /// Add a viewing key to the scanner without resetting state
    ///
    /// This allows registering new wallets without losing the current tree state,
    /// notes, and spent nullifiers. Use this instead of recreating the scanner.
    pub fn add_viewing_key(&mut self, viewing_key: OrchardViewingKey) {
        // Check if this key already exists (by wallet_id or account_index)
        let already_exists = self.viewing_keys.iter().any(|vk| {
            vk.wallet_id == viewing_key.wallet_id && vk.account_index == viewing_key.account_index
        });

        if already_exists {
            tracing::debug!(
                "[OrchardScanner] Viewing key already registered: wallet_id={:?}, account_index={}",
                viewing_key.wallet_id,
                viewing_key.account_index
            );
            return;
        }

        tracing::info!(
            "[OrchardScanner] Adding viewing key: wallet_id={:?}, account_index={}, birthday={}",
            viewing_key.wallet_id,
            viewing_key.account_index,
            viewing_key.birthday_height
        );

        self.viewing_keys.push(viewing_key);
    }

    /// Remove a viewing key from the scanner
    pub fn remove_viewing_key(&mut self, wallet_id: i32) {
        self.viewing_keys.retain(|vk| vk.wallet_id != Some(wallet_id));
        tracing::info!(
            "[OrchardScanner] Removed viewing key for wallet_id={}, remaining keys={}",
            wallet_id,
            self.viewing_keys.len()
        );
    }

    /// Get the number of registered viewing keys
    pub fn viewing_key_count(&self) -> usize {
        self.viewing_keys.len()
    }

    /// Append commitments to the tree without decryption
    ///
    /// This is an optimized method for witness refresh - it only updates the
    /// commitment tree without attempting to decrypt notes. Use this when you
    /// already know which notes are yours and just need to update their witnesses.
    ///
    /// Returns the number of commitments appended.
    pub fn append_commitments_only(&mut self, commitments: &[[u8; 32]], block_height: u64) -> Result<usize, super::OrchardError> {
        let count = commitments.len();

        for cmx in commitments {
            self.tree_tracker.append_commitment(cmx)
                .map_err(|e| super::OrchardError::Scanner(format!("Failed to append commitment: {}", e)))?;
        }

        self.tree_tracker.set_block_height(block_height);
        self.progress.last_scanned_height = block_height;

        Ok(count)
    }

    /// Append commitments and mark specific positions for witness tracking
    ///
    /// This is used when rebuilding tree state from a frontier. If we already
    /// know which positions contain our notes (from database), we can mark them
    /// without decryption.
    ///
    /// `mark_positions` is a sorted list of global tree positions to mark.
    /// `start_position` is the current tree position before appending.
    ///
    /// Returns the number of commitments appended.
    pub fn append_commitments_with_marks(
        &mut self,
        commitments: &[[u8; 32]],
        start_position: u64,
        mark_positions: &[u64],
        block_height: u64,
    ) -> Result<usize, super::OrchardError> {
        let count = commitments.len();
        let mut current_pos = start_position;
        let mark_set: std::collections::HashSet<u64> = mark_positions.iter().cloned().collect();

        for cmx in commitments {
            if mark_set.contains(&current_pos) {
                // This position contains our note - mark it for witness tracking
                self.tree_tracker.append_and_mark(cmx)
                    .map_err(|e| super::OrchardError::Scanner(format!("Failed to append and mark: {}", e)))?;
                tracing::debug!(
                    "[OrchardScanner] Marked position {} for witness tracking",
                    current_pos
                );
            } else {
                self.tree_tracker.append_commitment(cmx)
                    .map_err(|e| super::OrchardError::Scanner(format!("Failed to append commitment: {}", e)))?;
            }
            current_pos += 1;
        }

        self.tree_tracker.set_block_height(block_height);
        self.progress.last_scanned_height = block_height;

        Ok(count)
    }

    /// Get mutable access to tree tracker for witness operations
    pub fn tree_tracker_mut(&mut self) -> &mut OrchardTreeTracker {
        &mut self.tree_tracker
    }
}

/// Compact block data for scanning
#[derive(Debug, Clone)]
pub struct CompactBlock {
    /// Block height
    pub height: u64,
    /// Block hash
    pub hash: [u8; 32],
    /// Transactions in this block
    pub transactions: Vec<CompactTransaction>,
}

/// Compact transaction data
#[derive(Debug, Clone)]
pub struct CompactTransaction {
    /// Transaction hash
    pub hash: String,
    /// Actions of the old Orchard pool (`tx.orchard` in getblock output)
    pub orchard_actions: Vec<CompactOrchardAction>,
    /// Actions of the Ironwood pool (`tx.ironwood`, v6 transactions from NU6.3
    /// onward). Kept separate from `orchard_actions`: the two pools have their
    /// own commitment trees, so their commitments must never be appended to the
    /// same tree, and their notes decrypt under different domains.
    pub ironwood_actions: Vec<CompactOrchardAction>,
}

impl CompactTransaction {
    /// Actions belonging to one pool.
    pub fn actions_for(&self, pool: ShieldedPool) -> &[CompactOrchardAction] {
        match pool {
            ShieldedPool::Ironwood => &self.ironwood_actions,
            _ => &self.orchard_actions,
        }
    }
}

/// Compact Orchard action data
#[derive(Debug, Clone)]
pub struct CompactOrchardAction {
    /// Note commitment
    pub cmx: [u8; 32],
    /// Nullifier
    pub nullifier: [u8; 32],
    /// Ephemeral key
    pub ephemeral_key: [u8; 32],
    /// Encrypted note ciphertext
    pub ciphertext: Vec<u8>,
}


/// Balance breakdown by pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShieldedBalance {
    /// Total balance in zatoshis
    pub total_zatoshis: u64,

    /// Spendable balance (confirmed) in zatoshis
    pub spendable_zatoshis: u64,

    /// Pending balance (unconfirmed) in zatoshis
    pub pending_zatoshis: u64,

    /// Number of unspent notes
    pub note_count: u32,

    /// Which pool this balance belongs to, or `None` when it is the total across
    /// all shielded pools.
    ///
    /// A migrating wallet holds funds in both the old Orchard pool and Ironwood,
    /// so an aggregate figure cannot honestly claim a single pool — it used to be
    /// labelled "orchard" even when every zatoshi sat in Ironwood. Per-pool
    /// figures come from `get_wallet_balances_by_pool`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pool: Option<ShieldedPool>,
}

impl ShieldedBalance {
    /// Create a balance summary for one pool
    pub fn new(pool: ShieldedPool, total: u64, spendable: u64, note_count: u32) -> Self {
        Self {
            total_zatoshis: total,
            spendable_zatoshis: spendable,
            pending_zatoshis: total - spendable,
            note_count,
            pool: Some(pool),
        }
    }

    /// Create a balance summary covering every shielded pool
    pub fn new_aggregate(total: u64, spendable: u64, note_count: u32) -> Self {
        Self {
            total_zatoshis: total,
            spendable_zatoshis: spendable,
            pending_zatoshis: total - spendable,
            note_count,
            pool: None,
        }
    }

    /// Get balance in ZEC (decimal)
    pub fn total_zec(&self) -> f64 {
        self.total_zatoshis as f64 / 100_000_000.0
    }

    /// Get spendable balance in ZEC
    pub fn spendable_zec(&self) -> f64 {
        self.spendable_zatoshis as f64 / 100_000_000.0
    }
}


    