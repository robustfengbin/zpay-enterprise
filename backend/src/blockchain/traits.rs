#![allow(dead_code)]

use async_trait::async_trait;
use rust_decimal::Decimal;

use crate::error::AppResult;

/// Represents a transfer request
#[derive(Debug, Clone)]
pub struct TransferParams {
    pub from_address: String,
    pub to_address: String,
    pub private_key: String,
    pub token: String,
    pub amount: Decimal,
    pub gas_price_gwei: Option<Decimal>,
    pub gas_limit: Option<u64>,
}

/// Represents gas estimation result with EIP-1559 parameters
#[derive(Debug, Clone)]
pub struct GasEstimate {
    pub gas_limit: u64,
    pub gas_price_gwei: Decimal,       // Legacy gas price for display
    pub estimated_fee_eth: Decimal,    // Estimated fee using max_fee
    // EIP-1559 specific fields
    pub base_fee_gwei: Option<Decimal>,
    pub priority_fee_gwei: Option<Decimal>,
    pub max_fee_gwei: Option<Decimal>,
}

/// Represents transaction status
#[derive(Debug, Clone, PartialEq)]
pub enum TxStatus {
    Pending,
    Confirmed { block_number: u64, gas_used: u64 },
    Failed { reason: String },
    NotFound,
}

/// Token balance information
#[derive(Debug, Clone)]
pub struct TokenBalance {
    pub symbol: String,
    pub balance: Decimal,
    pub contract_address: Option<String>,
}

/// UTXO (Unspent Transaction Output) for UTXO-based chains
#[derive(Debug, Clone)]
pub struct Utxo {
    /// Transaction ID (hex)
    pub txid: String,
    /// Output index
    pub output_index: u32,
    /// Script pubkey (hex)
    pub script: String,
    /// Value in smallest unit (zatoshis for Zcash, satoshis for Bitcoin)
    pub value: u64,
    /// Block height where this UTXO was created
    pub height: u64,
}

/// Abstract trait for blockchain clients
/// Implement this trait to add support for new chains
#[async_trait]
pub trait ChainClient: Send + Sync {
    /// Get the chain identifier (e.g., "ethereum", "bsc", "polygon")
    fn chain_id(&self) -> &str;

    /// Get the chain's display name
    fn chain_name(&self) -> &str;

    /// Get the native token symbol (e.g., "ETH", "BNB")
    fn native_token_symbol(&self) -> &str;

    /// Get native token balance for an address
    async fn get_native_balance(&self, address: &str) -> AppResult<Decimal>;

    /// Get ERC20/BEP20 token balance
    async fn get_token_balance(&self, address: &str, token_symbol: &str) -> AppResult<Decimal>;

    /// Get all supported token balances for an address
    async fn get_all_balances(&self, address: &str) -> AppResult<(Decimal, Vec<TokenBalance>)>;

    /// Estimate gas for a transfer
    async fn estimate_gas(&self, params: &TransferParams) -> AppResult<GasEstimate>;

    /// Execute a native token transfer
    async fn transfer_native(&self, params: &TransferParams) -> AppResult<String>;

    /// Execute an ERC20/BEP20 token transfer
    async fn transfer_token(&self, params: &TransferParams) -> AppResult<String>;

    /// Get transaction status
    async fn get_tx_status(&self, tx_hash: &str) -> AppResult<TxStatus>;

    /// Validate an address format
    fn validate_address(&self, address: &str) -> bool;

    /// Get current gas price in Gwei
    async fn get_gas_price(&self) -> AppResult<Decimal>;

    /// Import address for tracking (used by UTXO-based chains like Zcash)
    /// Default implementation does nothing (not needed for account-based chains like Ethereum)
    async fn import_address_for_tracking(&self, _address: &str, _label: &str) -> AppResult<()> {
        Ok(())
    }

    /// Get current block height
    /// Default implementation returns 0 (should be overridden for chains that need this)
    async fn get_block_height(&self) -> AppResult<u64> {
        Ok(0)
    }

    /// Resolve a unix timestamp (seconds) to a block height for disclosure
    /// range scoping.  Default returns NotImplemented — chains without
    /// per-block timestamps (or where the use-case doesn't apply) should
    /// stay on the default; the caller falls back to height-only scoping.
    async fn block_at_timestamp(&self, _timestamp: i64) -> AppResult<u64> {
        Err(crate::error::AppError::NotImplemented(
            "block_at_timestamp not supported for this chain".to_string(),
        ))
    }

    /// Broadcast a raw signed transaction
    /// Default implementation returns an error (should be overridden for chains that support this)
    async fn broadcast_raw_transaction(&self, _raw_tx_hex: &str) -> AppResult<String> {
        Err(crate::error::AppError::NotImplemented(
            "Raw transaction broadcast not supported for this chain".to_string(),
        ))
    }

    /// Get UTXOs for an address (used by UTXO-based chains like Zcash, Bitcoin)
    /// Default implementation returns empty vec (not applicable for account-based chains)
    async fn get_utxos(&self, _address: &str) -> AppResult<Vec<Utxo>> {
        Ok(vec![])
    }

    /// Get the RPC URL for this chain
    /// Default implementation returns None (not all chains have RPC URLs exposed)
    async fn get_rpc_url(&self) -> Option<String> {
        None
    }

    /// Get RPC authentication credentials (user, password)
    /// Default implementation returns None (not all chains require auth)
    async fn get_rpc_auth(&self) -> Option<(String, String)> {
        None
    }

    /// Consensus network moniker for shielded (Zcash) address derivation:
    /// "main" | "test" | "regtest". Read live from the node so address HRPs
    /// (t-addr prefix, unified-address HRP) match the chain the RPC endpoint
    /// currently points at. Default "main" for chains that don't override.
    async fn get_shielded_network(&self) -> AppResult<String> {
        Ok("main".to_string())
    }

    /// Orchard (NU5) activation height, read live from the node's
    /// getblockchaininfo `upgrades`. Scanning / frontier init must start at or
    /// after this height; hardcoding mainnet 1,687,104 breaks short
    /// regtest/testnet chains ("block not in main chain"). Default is the
    /// mainnet constant, used only when a node doesn't expose it.
    async fn get_shielded_activation_height(&self) -> AppResult<u64> {
        Ok(1_687_104)
    }

    /// Send a shielded/privacy transfer (used by Zcash)
    /// Default implementation returns an error (not applicable for non-privacy chains)
    async fn send_shielded(
        &self,
        _from_address: &str,
        _to_address: &str,
        _amount: Decimal,
        _memo: Option<String>,
    ) -> AppResult<String> {
        Err(crate::error::AppError::NotImplemented(
            "Shielded transfers not supported for this chain".to_string(),
        ))
    }
}
