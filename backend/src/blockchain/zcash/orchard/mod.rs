//! Zcash Orchard privacy protocol implementation
//!
//! This module provides support for Zcash's Orchard shielded pool,
//! using the Halo 2 proving system for trustless privacy.

#![allow(dead_code)]

pub mod address;
pub mod builder;
pub mod keys;
pub mod scanner;
pub mod sync;
pub mod transfer;
pub mod tree;
pub mod turnstile;
pub mod witness_sync;

pub use address::UnifiedAddressInfo;
pub use builder::{OrchardTransactionBuilder, OrchardTransferParams};
pub use keys::OrchardViewingKey;
pub use scanner::ScanProgress;
pub use transfer::init_proving_key;

/// Orchard protocol constants
pub mod constants {
    /// Minimum confirmations before considering a note spendable
    pub const MIN_CONFIRMATIONS: u32 = 10;

    /// Orchard anchor depth for security
    pub const ANCHOR_OFFSET: u32 = 10;

    /// Default fee for Orchard transactions (in zatoshis)
    /// Orchard actions are more expensive than transparent transactions
    pub const DEFAULT_FEE_ZATOSHIS: u64 = 10000;

    /// ZIP 317 fee calculation constants
    pub const MARGINAL_FEE_ZATOSHIS: u64 = 5000;
    pub const GRACE_ACTIONS: u32 = 2;
    pub const P2PKH_STANDARD_INPUT_SIZE: u64 = 150;
    pub const P2PKH_STANDARD_OUTPUT_SIZE: u64 = 34;
}

/// Error types for Orchard operations
#[derive(Debug, thiserror::Error)]
pub enum OrchardError {
    #[error("Key derivation failed: {0}")]
    KeyDerivation(String),

    #[error("Address generation failed: {0}")]
    AddressGeneration(String),

    #[error("Transaction building failed: {0}")]
    TransactionBuild(String),

    #[error("Proof generation failed: {0}")]
    ProofGeneration(String),

    #[error("Note decryption failed: {0}")]
    NoteDecryption(String),

    #[error("Insufficient shielded balance: have {available} zatoshis, need {required} zatoshis")]
    InsufficientBalance { available: u64, required: u64 },

    #[error("No spendable notes found")]
    NoSpendableNotes,

    #[error("Witness not found for note")]
    WitnessNotFound,

    #[error("Scanner error: {0}")]
    Scanner(String),

    #[error("Invalid unified address: {0}")]
    InvalidUnifiedAddress(String),

    #[error("RPC error: {0}")]
    RpcError(String),

    #[error("Database error: {0}")]
    DatabaseError(String),
}

impl From<OrchardError> for crate::error::AppError {
    fn from(err: OrchardError) -> Self {
        crate::error::AppError::BlockchainError(err.to_string())
    }
}

impl From<tree::TreeError> for OrchardError {
    fn from(err: tree::TreeError) -> Self {
        OrchardError::Scanner(err.to_string())
    }
}

/// Result type for Orchard operations
pub type OrchardResult<T> = Result<T, OrchardError>;

/// Shielded pool type indicator
///
/// `Orchard` and `Ironwood` are the two Orchard-protocol pools this wallet
/// tracks. They share the Action/Halo2 wire format but are *type-distinct* on
/// chain: separate note commitment trees, separate nullifier sets, separate
/// chain value pools, and different note plaintext versions (V2 vs V3/rcm_v3).
/// A note therefore only ever belongs to one of them, and spends must never mix
/// the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShieldedPool {
    /// Orchard pool (Halo 2). Turnstile-closed from NU6.3 (spend-only).
    #[default]
    Orchard,
    /// Ironwood pool (NU6.3 onward, ZIP-2005 rcm_v3 notes)
    Ironwood,
    /// Sapling pool (Groth16)
    Sapling,
}

impl ShieldedPool {
    /// The value stored in `orchard_notes.pool` / `orchard_tree_state.pool`.
    pub fn as_db_str(&self) -> &'static str {
        match self {
            ShieldedPool::Orchard => "orchard",
            ShieldedPool::Ironwood => "ironwood",
            ShieldedPool::Sapling => "sapling",
        }
    }

    /// Parse a `pool` column value. Unknown values fall back to the Orchard
    /// pool, which is what the column default backfilled existing rows to.
    pub fn from_db_str(s: &str) -> Self {
        match s {
            "ironwood" => ShieldedPool::Ironwood,
            "sapling" => ShieldedPool::Sapling,
            _ => ShieldedPool::Orchard,
        }
    }

    /// The two note pools a Zcash wallet scans, in scan order.
    pub const NOTE_POOLS: [ShieldedPool; 2] = [ShieldedPool::Orchard, ShieldedPool::Ironwood];
}

impl std::fmt::Display for ShieldedPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_db_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_db_str_round_trip() {
        for pool in [
            ShieldedPool::Orchard,
            ShieldedPool::Ironwood,
            ShieldedPool::Sapling,
        ] {
            assert_eq!(ShieldedPool::from_db_str(pool.as_db_str()), pool);
        }
    }

    #[test]
    fn test_pool_defaults_to_orchard() {
        // Notes written before the pool column existed were backfilled to
        // 'orchard'; anything unrecognised must read the same way, never as
        // Ironwood (which would send a spend to the wrong tree).
        assert_eq!(ShieldedPool::default(), ShieldedPool::Orchard);
        assert_eq!(ShieldedPool::from_db_str(""), ShieldedPool::Orchard);
        assert_eq!(ShieldedPool::from_db_str("Ironwood"), ShieldedPool::Orchard);
        assert_eq!(ShieldedPool::from_db_str("nonsense"), ShieldedPool::Orchard);
    }

    #[test]
    fn test_note_pools_are_the_two_scanned_pools() {
        assert_eq!(
            ShieldedPool::NOTE_POOLS,
            [ShieldedPool::Orchard, ShieldedPool::Ironwood]
        );
    }
}
