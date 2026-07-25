//! Orchard commitment tree tracking for witness management
//!
//! This module implements proper commitment tree tracking for Orchard notes,
//! enabling shielded spending by providing valid Merkle witnesses.
//!
//! Key concepts:
//! - Commitment tree: A Merkle tree of all note commitments (cmx values)
//! - Anchor: The root of the commitment tree at a specific block height
//! - Witness: Authentication path from a note's position to the anchor
//!
//! For spending shielded notes, we need to prove the note exists in the tree
//! by providing a valid witness that computes to the anchor.

#![allow(dead_code)]

use incrementalmerkletree::{
    frontier::CommitmentTree,
    witness::IncrementalWitness,
    Hashable,
};
use orchard::tree::{MerkleHashOrchard, MerklePath as OrchardMerklePath};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Cursor;
use subtle::CtOption;
use zcash_primitives::merkle_tree::{
    read_commitment_tree, write_commitment_tree,
    read_incremental_witness, write_incremental_witness,
};

/// Orchard tree depth
pub const ORCHARD_TREE_DEPTH: u8 = 32;

/// Orchard commitment tree tracker
///
/// Tracks the global Orchard commitment tree and maintains witnesses
/// for notes that belong to our wallets.
#[derive(Clone)]
pub struct OrchardTreeTracker {
    /// The commitment tree state
    tree: CommitmentTree<MerkleHashOrchard, ORCHARD_TREE_DEPTH>,

    /// Witnesses for our notes, keyed by position
    /// Each witness is updated as new commitments are added
    witnesses: HashMap<u64, IncrementalWitness<MerkleHashOrchard, ORCHARD_TREE_DEPTH>>,

    /// Current position (number of commitments added)
    current_position: u64,

    /// Block height of the last update
    last_block_height: u64,
}

impl Default for OrchardTreeTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl OrchardTreeTracker {
    /// Create a new empty tree tracker
    pub fn new() -> Self {
        Self {
            tree: CommitmentTree::empty(),
            witnesses: HashMap::new(),
            current_position: 0,
            last_block_height: 0,
        }
    }

    ///
    /// The frontier is obtained from z_gettreestate RPC.
    /// This allows us to continue building from a known tree state
    /// without scanning from genesis.
    pub fn from_frontier(frontier_hex: &str, position: u64, block_height: u64) -> Result<Self, TreeError> {
        let frontier_bytes = hex::decode(frontier_hex)
            .map_err(|e| TreeError::InvalidCommitment(format!("Invalid frontier hex: {}", e)))?;

        // Parse the frontier using zcash_primitives format
        let tree: CommitmentTree<MerkleHashOrchard, ORCHARD_TREE_DEPTH> =
            read_commitment_tree(&mut Cursor::new(&frontier_bytes))
                .map_err(|e| TreeError::InvalidCommitment(format!("Failed to parse frontier: {}", e)))?;

        tracing::info!(
            "[OrchardTree] Initialized from frontier at height {}, position {}, tree_size={}",
            block_height,
            position,
            tree.size()
        );

        // See reset_from_frontier: z_gettreestate reports no leaf count, so a
        // caller-supplied 0 means "ask the frontier".
        let current_position = if position == 0 {
            tree.size() as u64
        } else {
            position
        };

        Ok(Self {
            tree,
            witnesses: HashMap::new(),
            current_position,
            last_block_height: block_height,
        })
    }

    /// Reset and initialize from frontier
    pub fn reset_from_frontier(&mut self, frontier_hex: &str, position: u64, block_height: u64) -> Result<(), TreeError> {
        let frontier_bytes = hex::decode(frontier_hex)
            .map_err(|e| TreeError::InvalidCommitment(format!("Invalid frontier hex: {}", e)))?;

        self.tree = read_commitment_tree(&mut Cursor::new(&frontier_bytes))
            .map_err(|e| TreeError::InvalidCommitment(format!("Failed to parse frontier: {}", e)))?;

        self.witnesses.clear();
        // z_gettreestate reports only finalState/finalRoot — no leaf count — so
        // callers pass 0 and the frontier itself is the authority on how many
        // commitments precede us. Starting a non-empty frontier at position 0
        // would number every note we then discover from 0 instead of its true
        // global tree position.
        self.current_position = if position == 0 {
            self.tree.size() as u64
        } else {
            position
        };
        self.last_block_height = block_height;

        tracing::info!(
            "[OrchardTree] Reset from frontier at height {}, position {}, tree_size={}",
            block_height,
            position,
            self.tree.size()
        );

        Ok(())
    }

    /// Serialize the current tree state to bytes
    pub fn serialize(&self) -> Result<Vec<u8>, TreeError> {
        let mut buffer = Vec::new();
        write_commitment_tree(&self.tree, &mut buffer)
            .map_err(|e| TreeError::InvalidCommitment(format!("Failed to serialize tree: {}", e)))?;
        Ok(buffer)
    }

    /// Append a commitment to the tree
    ///
    /// Returns the position of the new commitment
    pub fn append_commitment(&mut self, cmx: &[u8; 32]) -> Result<u64, TreeError> {
        // Parse the commitment as a MerkleHashOrchard
        let hash = parse_merkle_hash(cmx)?;

        // Append to tree
        self.tree.append(hash.clone())
            .map_err(|_| TreeError::TreeFull)?;

        let position = self.current_position;
        self.current_position += 1;

        // Update all existing witnesses
        for witness in self.witnesses.values_mut() {
            witness.append(hash.clone())
                .map_err(|_| TreeError::WitnessUpdateFailed)?;
        }

        Ok(position)
    }

    /// Append a commitment and mark it for witness tracking
    ///
    /// Use this when we discover a note that belongs to us.
    /// The witness will be tracked and updated as new commitments are added.
    pub fn append_and_mark(&mut self, cmx: &[u8; 32]) -> Result<u64, TreeError> {
        // Parse the commitment
        let hash = parse_merkle_hash(cmx)?;

        // Append to tree
        self.tree.append(hash.clone())
            .map_err(|_| TreeError::TreeFull)?;

        let position = self.current_position;
        self.current_position += 1;

        // Update existing witnesses first
        for witness in self.witnesses.values_mut() {
            witness.append(hash.clone())
                .map_err(|_| TreeError::WitnessUpdateFailed)?;
        }

        // Create a new witness for this position
        let witness = IncrementalWitness::from_tree(self.tree.clone())
            .ok_or(TreeError::WitnessUpdateFailed)?;
        self.witnesses.insert(position, witness);

        tracing::debug!(
            "[OrchardTree] Marked position {} for witness tracking (total witnesses: {})",
            position,
            self.witnesses.len()
        );

        Ok(position)
    }

    /// Mark an existing position for witness tracking
    ///
    /// Note: This only works if the tree state at that position was preserved.
    /// In practice, you should call `append_and_mark` when discovering notes.
    pub fn mark_position(&mut self, position: u64) -> Result<(), TreeError> {
        if position >= self.current_position {
            return Err(TreeError::InvalidPosition(position));
        }

        // We can't retroactively create a witness for a past position
        // without the tree state at that point.
        // This method is mainly a placeholder - real witness creation
        // happens during scanning via append_and_mark.

        tracing::warn!(
            "[OrchardTree] Cannot retroactively mark position {}. \
             Witnesses must be created during scanning.",
            position
        );

        Err(TreeError::CannotMarkPastPosition)
    }

    /// Get the current tree root (anchor)
    pub fn root(&self) -> [u8; 32] {
        if self.current_position == 0 {
            // Empty tree root
            let empty_root = MerkleHashOrchard::empty_root(
                incrementalmerkletree::Level::from(ORCHARD_TREE_DEPTH)
            );
            empty_root.to_bytes()
        } else {
            self.tree.root().to_bytes()
        }
    }

    /// Get the witness (Merkle path) for a position
    pub fn get_witness(&self, position: u64) -> Option<WitnessData> {
        let witness = self.witnesses.get(&position)?;
        let path = witness.path()?;

        // Use the position from the path, not from our HashMap key
        // The path's position is the actual position in the tree
        let path_position: u64 = path.position().into();

        // Log if there's a mismatch (shouldn't happen but useful for debugging)
        if path_position != position {
            tracing::warn!(
                "[OrchardTree] Position mismatch: HashMap key={}, path.position={}",
                position,
                path_position
            );
        }

        Some(WitnessData {
            position: path_position,  // Use position from path
            auth_path: path.path_elems()
                .iter()
                .map(|h| h.to_bytes())
                .collect(),
            root: witness.root().to_bytes(),
        })
    }

    /// Convert witness data to orchard::tree::MerklePath
    pub fn get_orchard_merkle_path(&self, position: u64) -> Option<OrchardMerklePath> {
        let witness = self.witnesses.get(&position)?;
        let path = witness.path()?;

        // Convert incrementalmerkletree::MerklePath to orchard::tree::MerklePath
        Some(OrchardMerklePath::from(path))
    }

    /// Get the current anchor as an orchard::tree::Anchor
    pub fn get_anchor(&self) -> orchard::tree::Anchor {
        let root = self.root();
        // Parse root bytes to create anchor
        let hash_opt: CtOption<MerkleHashOrchard> = MerkleHashOrchard::from_bytes(&root);
        if hash_opt.is_some().into() {
            orchard::tree::Anchor::from(hash_opt.unwrap())
        } else {
            orchard::tree::Anchor::empty_tree()
        }
    }

    /// Get the current position count
    pub fn position(&self) -> u64 {
        self.current_position
    }

    /// Get the tree size (number of commitments in the tree)
    pub fn tree_size(&self) -> usize {
        self.tree.size()
    }

    /// Set the last processed block height
    pub fn set_block_height(&mut self, height: u64) {
        self.last_block_height = height;
    }

    /// Get the last processed block height
    pub fn block_height(&self) -> u64 {
        self.last_block_height
    }

    /// Get the number of tracked witnesses
    pub fn witness_count(&self) -> usize {
        self.witnesses.len()
    }

    /// Remove a witness (e.g., when a note is spent)
    pub fn remove_witness(&mut self, position: u64) -> bool {
        self.witnesses.remove(&position).is_some()
    }

    /// Serialize the tree state for persistence
    pub fn to_state(&self) -> TreeState {
        TreeState {
            position: self.current_position,
            block_height: self.last_block_height,
            // Note: Full tree serialization would be complex
            // For now, we'll rely on rescanning from a checkpoint
        }
    }

    // =========================================================================
    // Serialization methods for incremental sync
    // =========================================================================

    /// Serialize the commitment tree to bytes
    pub fn serialize_tree(&self) -> Result<Vec<u8>, TreeError> {
        let mut buffer = Vec::new();
        write_commitment_tree(&self.tree, &mut buffer)
            .map_err(|e| TreeError::InvalidCommitment(format!("Failed to serialize tree: {}", e)))?;
        Ok(buffer)
    }

    /// Deserialize and restore the commitment tree from bytes
    pub fn deserialize_tree(data: &[u8]) -> Result<CommitmentTree<MerkleHashOrchard, ORCHARD_TREE_DEPTH>, TreeError> {
        read_commitment_tree(&mut Cursor::new(data))
            .map_err(|e| TreeError::InvalidCommitment(format!("Failed to deserialize tree: {}", e)))
    }

    /// Restore tree tracker from serialized data
    pub fn from_serialized(
        tree_data: &[u8],
        position: u64,
        block_height: u64,
    ) -> Result<Self, TreeError> {
        let tree = Self::deserialize_tree(tree_data)?;

        tracing::info!(
            "[OrchardTree] Restored from serialized data: height={}, position={}, tree_size={}",
            block_height,
            position,
            tree.size()
        );

        Ok(Self {
            tree,
            witnesses: HashMap::new(),
            current_position: position,
            last_block_height: block_height,
        })
    }

    /// Serialize a single witness to bytes
    pub fn serialize_witness(witness: &IncrementalWitness<MerkleHashOrchard, ORCHARD_TREE_DEPTH>) -> Result<Vec<u8>, TreeError> {
        let mut buffer = Vec::new();
        write_incremental_witness(witness, &mut buffer)
            .map_err(|e| TreeError::InvalidWitness(format!("Failed to serialize witness: {}", e)))?;
        Ok(buffer)
    }

    /// Deserialize a witness from bytes
    pub fn deserialize_witness(data: &[u8]) -> Result<IncrementalWitness<MerkleHashOrchard, ORCHARD_TREE_DEPTH>, TreeError> {
        read_incremental_witness(&mut Cursor::new(data))
            .map_err(|e| TreeError::InvalidWitness(format!("Failed to deserialize witness: {}", e)))
    }

    /// Get serialized witness for a position
    pub fn get_serialized_witness(&self, position: u64) -> Option<Vec<u8>> {
        let witness = self.witnesses.get(&position)?;
        Self::serialize_witness(witness).ok()
    }

    /// Add a pre-existing witness (for restoring from database)
    pub fn add_witness(&mut self, position: u64, witness: IncrementalWitness<MerkleHashOrchard, ORCHARD_TREE_DEPTH>) {
        self.witnesses.insert(position, witness);
    }

    /// Add a witness from serialized data
    pub fn add_serialized_witness(&mut self, position: u64, data: &[u8]) -> Result<(), TreeError> {
        let witness = Self::deserialize_witness(data)?;
        self.witnesses.insert(position, witness);
        Ok(())
    }

    /// Create a new IncrementalWitness from current tree state
    /// This is used when discovering new notes or restoring known positions
    pub fn create_witness_from_current(&self) -> Option<IncrementalWitness<MerkleHashOrchard, ORCHARD_TREE_DEPTH>> {
        IncrementalWitness::from_tree(self.tree.clone())
    }

    /// Get all witnesses as serialized data (for persistence)
    pub fn get_all_serialized_witnesses(&self) -> Result<Vec<(u64, Vec<u8>)>, TreeError> {
        let mut result = Vec::new();
        for (position, witness) in &self.witnesses {
            let data = Self::serialize_witness(witness)?;
            result.push((*position, data));
        }
        Ok(result)
    }

    /// Get witness data and serialized state for a position
    pub fn get_witness_with_state(&self, position: u64) -> Option<(WitnessData, Vec<u8>)> {
        let witness = self.witnesses.get(&position)?;
        let path = witness.path()?;
        let path_position: u64 = path.position().into();

        let witness_data = WitnessData {
            position: path_position,
            auth_path: path.path_elems().iter().map(|h| h.to_bytes()).collect(),
            root: witness.root().to_bytes(),
        };

        let serialized = Self::serialize_witness(witness).ok()?;
        Some((witness_data, serialized))
    }
}

/// Witness data for a single note
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WitnessData {
    /// Position of the note in the commitment tree
    pub position: u64,
    /// Authentication path (32 hashes for depth 32)
    pub auth_path: Vec<[u8; 32]>,
    /// Tree root at the time the witness was computed
    pub root: [u8; 32],
}

impl WitnessData {
    /// Convert to orchard::tree::MerklePath
    pub fn to_merkle_path(&self) -> Result<OrchardMerklePath, TreeError> {
        if self.auth_path.len() != ORCHARD_TREE_DEPTH as usize {
            return Err(TreeError::InvalidWitness(format!(
                "Expected {} auth path elements, got {}",
                ORCHARD_TREE_DEPTH,
                self.auth_path.len()
            )));
        }

        let mut auth_path = [MerkleHashOrchard::empty_leaf(); 32];
        for (i, hash_bytes) in self.auth_path.iter().enumerate() {
            let hash_opt: CtOption<MerkleHashOrchard> = MerkleHashOrchard::from_bytes(hash_bytes);
            if hash_opt.is_some().into() {
                auth_path[i] = hash_opt.unwrap();
            } else {
                return Err(TreeError::InvalidWitness(format!(
                    "Invalid hash at position {}",
                    i
                )));
            }
        }

        Ok(OrchardMerklePath::from_parts(self.position as u32, auth_path))
    }
}

/// Serializable tree state for persistence
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeState {
    /// Current position (number of commitments)
    pub position: u64,
    /// Last block height processed
    pub block_height: u64,
}

/// Tree error types
#[derive(Debug, Clone)]
pub enum TreeError {
    /// The commitment tree is full (2^32 leaves)
    TreeFull,
    /// Failed to parse commitment bytes
    InvalidCommitment(String),
    /// Position is out of range
    InvalidPosition(u64),
    /// Cannot mark a past position (must mark during scanning)
    CannotMarkPastPosition,
    /// Failed to update witness
    WitnessUpdateFailed,
    /// Invalid witness data
    InvalidWitness(String),
}

impl std::fmt::Display for TreeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TreeError::TreeFull => write!(f, "Commitment tree is full"),
            TreeError::InvalidCommitment(msg) => write!(f, "Invalid commitment: {}", msg),
            TreeError::InvalidPosition(pos) => write!(f, "Invalid position: {}", pos),
            TreeError::CannotMarkPastPosition => write!(f, "Cannot mark past position"),
            TreeError::WitnessUpdateFailed => write!(f, "Failed to update witness"),
            TreeError::InvalidWitness(msg) => write!(f, "Invalid witness: {}", msg),
        }
    }
}

impl std::error::Error for TreeError {}

/// Parse bytes to MerkleHashOrchard
fn parse_merkle_hash(bytes: &[u8; 32]) -> Result<MerkleHashOrchard, TreeError> {
    let hash_opt: CtOption<MerkleHashOrchard> = MerkleHashOrchard::from_bytes(bytes);
    if hash_opt.is_some().into() {
        Ok(hash_opt.unwrap())
    } else {
        Err(TreeError::InvalidCommitment(format!(
            "Failed to parse commitment: {}",
            hex::encode(bytes)
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tree() {
        let tracker = OrchardTreeTracker::new();
        assert_eq!(tracker.position(), 0);

        // Empty tree should have the canonical empty root
        let root = tracker.root();
        let anchor = tracker.get_anchor();
        assert_eq!(anchor, orchard::tree::Anchor::empty_tree());
    }

    #[test]
    fn test_append_commitment() {
        let mut tracker = OrchardTreeTracker::new();

        // Create a valid commitment (must be a valid pallas::Base element)
        // Using a small value that's definitely in range
        let mut cmx = [0u8; 32];
        cmx[0] = 1;

        let pos = tracker.append_commitment(&cmx).unwrap();
        assert_eq!(pos, 0);
        assert_eq!(tracker.position(), 1);
    }

    #[test]
    fn test_append_and_mark() {
        let mut tracker = OrchardTreeTracker::new();

        // Add some commitments without marking
        let mut cmx1 = [0u8; 32];
        cmx1[0] = 1;
        tracker.append_commitment(&cmx1).unwrap();

        let mut cmx2 = [0u8; 32];
        cmx2[0] = 2;
        tracker.append_commitment(&cmx2).unwrap();

        // Mark the third commitment
        let mut cmx3 = [0u8; 32];
        cmx3[0] = 3;
        let pos = tracker.append_and_mark(&cmx3).unwrap();

        assert_eq!(pos, 2);
        assert_eq!(tracker.witness_count(), 1);

        // Add more commitments to update the witness
        let mut cmx4 = [0u8; 32];
        cmx4[0] = 4;
        tracker.append_commitment(&cmx4).unwrap();

        // We should be able to get a witness
        let witness = tracker.get_witness(2);
        assert!(witness.is_some());
    }

    // =====================================================================
    // F4.0-c dual-pool: the two pools' commitments build two independent
    // trees. These vectors are the *real* chain data from regtest 档B′ (the
    // W4 turnstile run, NU6.3 active at 500): every Orchard and Ironwood
    // commitment on that chain up to height 520, in append order, with the
    // roots the node itself reports via `z_gettreestate(520)`.
    //
    // Building each pool's tree from its own commitments must reproduce that
    // pool's root exactly. This is what guarantees a spend's anchor is
    // accepted: an Ironwood commitment appended to the Orchard tree (or vice
    // versa) would silently shift both roots and every proof would fail.
    // =====================================================================

    /// `tx.orchard` commitments of 档B′ blocks 1..=520 (cmx, append order)
    const ORCHARD_CMX: [&str; 6] = [
        "2f415b070c52de9bc713fd6420cc38ca57bf2b7de837077252c0c67e65dd8905",
        "bb4d592e5abaf0e2338e7353183f8757d7339e8c10241f5fd953e02a27b2360e",
        "af56c4f4e1e0a708913eab5767f280e4c88a562a3dc20bbec65e7e29fee20d10",
        "bcc6f3c9e47ede2774f085934eea50d9e57470a5ae27cf7284965945a9636810",
        "6a0a3192bcc23c8640c2da81ca88790a5c5aa1e2969faa191696091a9e7cf82c",
        "1e643a40755cfa01bf3f16ee44a8737a14b00d62962f4a68f3c7d14eb836cb39",
    ];
    const ORCHARD_ROOT: &str =
        "5d7420bbc7bb61276cfa9370777320efbd2194968ad19808252dabd364809637";

    /// `tx.ironwood` commitments of the same chain — only the two turnstile
    /// transactions (blocks 511 and 518) produced any.
    const IRONWOOD_CMX: [&str; 4] = [
        "bd9406ce2d0d1663fa456ff7503555c2e9da418d24e4bf27096a7b516b461e13",
        "e3cff64ce5a1d11727a3867286ee348b8cfcd5882c68c93f2dc7fbc0ff6c5f0c",
        "7eb893491480434a84ee03610b95022a663457c3fab90546bacacab6a5680d09",
        "4e96517e5eb34a7ead0fd9cc6fe95e2488e981752949e43257108004f32d1613",
    ];
    const IRONWOOD_ROOT: &str =
        "276296ef3aa999c46f5eb98b1f1d536328368ea401ff62adeb9b4106a7815704";

    fn tree_from_hex_commitments(commitments: &[&str]) -> OrchardTreeTracker {
        let mut tracker = OrchardTreeTracker::new();
        for cmx_hex in commitments {
            let bytes = hex::decode(cmx_hex).expect("valid cmx hex");
            let mut cmx = [0u8; 32];
            cmx.copy_from_slice(&bytes);
            tracker.append_commitment(&cmx).expect("append cmx");
        }
        tracker
    }

    #[test]
    fn test_orchard_tree_matches_node_root() {
        let tracker = tree_from_hex_commitments(&ORCHARD_CMX);
        assert_eq!(tracker.position(), ORCHARD_CMX.len() as u64);
        assert_eq!(hex::encode(tracker.root()), ORCHARD_ROOT);
    }

    #[test]
    fn test_ironwood_tree_matches_node_root() {
        let tracker = tree_from_hex_commitments(&IRONWOOD_CMX);
        assert_eq!(tracker.position(), IRONWOOD_CMX.len() as u64);
        assert_eq!(hex::encode(tracker.root()), IRONWOOD_ROOT);
    }

    /// The two pools must stay separate: feeding one pool's commitments into
    /// the other pool's tree (what a single-tree scanner would do) yields a
    /// root the node would reject as an unknown anchor.
    #[test]
    fn test_mixing_pools_breaks_both_roots() {
        let mut mixed: Vec<&str> = ORCHARD_CMX.to_vec();
        mixed.extend_from_slice(&IRONWOOD_CMX);
        let tracker = tree_from_hex_commitments(&mixed);

        let mixed_root = hex::encode(tracker.root());
        assert_ne!(mixed_root, ORCHARD_ROOT);
        assert_ne!(mixed_root, IRONWOOD_ROOT);
    }

    /// A frontier restored with no leaf count (z_gettreestate reports none)
    /// must take its position from the frontier itself, not start over at 0.
    #[test]
    fn test_frontier_position_derived_from_tree_size() {
        let full = tree_from_hex_commitments(&ORCHARD_CMX);
        let frontier = hex::encode(full.serialize_tree().expect("serialize"));

        let mut restored = OrchardTreeTracker::new();
        restored
            .reset_from_frontier(&frontier, 0, 520)
            .expect("reset from frontier");

        assert_eq!(restored.position(), ORCHARD_CMX.len() as u64);
        assert_eq!(hex::encode(restored.root()), ORCHARD_ROOT);
    }
}
