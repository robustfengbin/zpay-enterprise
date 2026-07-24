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
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ShieldedPool {
    /// Orchard pool (Halo 2)
    Orchard,
    /// Sapling pool (Groth16)
    Sapling,
}

impl std::fmt::Display for ShieldedPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShieldedPool::Orchard => write!(f, "orchard"),
            ShieldedPool::Sapling => write!(f, "sapling"),
        }
    }
}
