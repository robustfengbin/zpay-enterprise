#![allow(dead_code)]

use async_trait::async_trait;
use reqwest::Proxy;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use tokio::sync::RwLock;
use zcash_protocol::consensus::NetworkType;

use crate::blockchain::traits::{ChainClient, GasEstimate, TokenBalance, TransferParams, TxStatus, Utxo};
use crate::blockchain::zcash::orchard::{
    keys::OrchardKeyManager, scanner::{OrchardScanner, ShieldedBalance}, OrchardTransactionBuilder,
    OrchardTransferParams, OrchardViewingKey, ScanProgress, ShieldedPool,
};
use crate::config::ZcashConfig;
use crate::error::{AppError, AppResult};

/// Dynamic RPC configuration that can be updated at runtime
pub struct RpcSettings {
    pub primary_rpc: String,
    #[allow(dead_code)]
    pub fallback_rpcs: Vec<String>,
    pub rpc_proxy: Option<String>,
    pub rpc_user: Option<String>,
    pub rpc_password: Option<String>,
}

pub struct ZcashClient {
    rpc_settings: RwLock<RpcSettings>,
    /// Orchard scanner for shielded note detection
    orchard_scanner: RwLock<Option<OrchardScanner>>,
    /// Consensus network context (network + Orchard activation height) resolved
    /// live from the node. Bound to the current RPC endpoint: `update_rpc`
    /// clears it so a runtime endpoint switch re-reads the network and we never
    /// derive an address for the wrong chain.
    network_ctx: RwLock<Option<NetworkContext>>,
}

// JSON-RPC request/response types
#[derive(Debug, Serialize)]
struct JsonRpcRequest<T> {
    jsonrpc: &'static str,
    id: u64,
    method: &'static str,
    params: T,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse<T> {
    result: Option<T>,
    error: Option<JsonRpcError>,
    #[allow(dead_code)]
    id: u64,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

// Zcash specific types
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ValidateAddressResult {
    isvalid: bool,
    address: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct GetBalanceResult(f64);

#[derive(Debug, Deserialize)]
struct ListUnspentEntry {
    #[allow(dead_code)]
    txid: String,
    #[allow(dead_code)]
    vout: u32,
    address: String,
    amount: f64,
    confirmations: u32,
}

/// UTXO from getaddressutxos RPC (Zebra compatible)
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AddressUtxo {
    #[allow(dead_code)]
    address: String,
    txid: String,
    output_index: u32,
    script: String,
    satoshis: u64,
    #[allow(dead_code)]
    height: u64,
}

/// Blockchain info from getblockchaininfo RPC
#[derive(Debug, Deserialize)]
struct BlockchainInfo {
    /// Network moniker: "main" | "test" | "regtest". Absent on some legacy
    /// forks, so default to "main".
    #[serde(default = "default_chain_moniker")]
    chain: String,
    blocks: u64,
    consensus: ConsensusInfo,
    /// Network-upgrade table keyed by branch-id hex. Used to read the NU5
    /// (Orchard) activation height live from the node instead of hardcoding it.
    #[serde(default)]
    upgrades: HashMap<String, NetworkUpgradeInfo>,
}

fn default_chain_moniker() -> String {
    "main".to_string()
}

/// One entry of getblockchaininfo `upgrades`
#[derive(Debug, Deserialize)]
struct NetworkUpgradeInfo {
    name: String,
    #[serde(rename = "activationheight")]
    activation_height: u64,
}

impl BlockchainInfo {
    /// Map the node's chain moniker to a consensus network type.
    fn network_type(&self) -> NetworkType {
        match self.chain.as_str() {
            "test" => NetworkType::Test,
            "regtest" => NetworkType::Regtest,
            "main" => NetworkType::Main,
            other => {
                tracing::warn!(
                    "Unknown chain moniker '{}' from getblockchaininfo; defaulting to mainnet",
                    other
                );
                NetworkType::Main
            }
        }
    }

    /// NU5 (Orchard) activation height as reported by the node, if present.
    fn nu5_activation_height(&self) -> Option<u64> {
        self.upgrades
            .values()
            .find(|u| u.name.eq_ignore_ascii_case("nu5"))
            .map(|u| u.activation_height)
    }

    /// NU6.3 (Ironwood) activation height as reported by the node, if the
    /// upgrade is scheduled/known. `None` means the node has no NU6.3 entry
    /// (e.g. a NU6.2-only regtest), i.e. the turnstile is not in play here.
    /// Node monikers vary ("NU6.3" / "NU6_3"); match on the digits.
    fn nu63_activation_height(&self) -> Option<u64> {
        self.upgrades
            .values()
            .find(|u| {
                let n = u.name.to_ascii_lowercase();
                n == "nu6.3" || n == "nu6_3" || n == "nu63"
            })
            .map(|u| u.activation_height)
    }
}

/// Cached, endpoint-bound consensus context resolved from getblockchaininfo.
#[derive(Debug, Clone, Copy)]
struct NetworkContext {
    network: NetworkType,
    orchard_activation_height: u64,
    /// NU6.3 (Ironwood) activation height, or None if the node has no such
    /// upgrade scheduled. Drives the turnstile: below it the old Orchard pool
    /// semantics apply, at/above it spends cross into Ironwood.
    nu63_activation_height: Option<u64>,
}

/// Consensus info containing the current branch ID
#[derive(Debug, Deserialize)]
struct ConsensusInfo {
    /// Current chain tip consensus branch ID (hex string like "c8e71055")
    chaintip: String,
}

#[derive(Debug, Deserialize)]
struct GetTransactionResult {
    confirmations: Option<i64>,
    blockhash: Option<String>,
    #[allow(dead_code)]
    txid: String,
}

#[derive(Debug, Deserialize)]
struct GetBlockResult {
    height: u64,
    /// Unix timestamp (seconds) of the block.  Optional in the RPC schema
    /// for older Zcash forks; we treat absence as "unknown".
    #[serde(default)]
    time: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct EstimateFeeResult(f64);

/// z_sendmany recipient entry
#[derive(Debug, Clone, Serialize)]
struct ZSendManyRecipient {
    address: String,
    amount: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    memo: Option<String>,
}

/// z_getoperationstatus result
#[derive(Debug, Clone, Deserialize)]
struct ZOperationStatus {
    id: String,
    status: String,
    #[serde(default)]
    result: Option<ZOperationResult>,
    #[serde(default)]
    error: Option<ZOperationError>,
}

/// z_getoperationstatus result data
#[derive(Debug, Clone, Deserialize)]
struct ZOperationResult {
    txid: String,
}

/// z_getoperationstatus error data
#[derive(Debug, Clone, Deserialize)]
struct ZOperationError {
    code: i32,
    message: String,
}

impl ZcashClient {
    /// Create a reqwest client with optional proxy and basic auth support
    fn create_http_client(proxy_url: &Option<String>) -> AppResult<reqwest::Client> {
        let mut client_builder = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30));

        if let Some(proxy) = proxy_url {
            if !proxy.is_empty() {
                let proxy = Proxy::all(proxy)
                    .map_err(|e| AppError::BlockchainError(format!("Invalid proxy URL: {}", e)))?;
                client_builder = client_builder.proxy(proxy);
                tracing::debug!("Zcash RPC proxy configured");
            }
        }

        client_builder
            .build()
            .map_err(|e| AppError::BlockchainError(format!("Failed to create HTTP client: {}", e)))
    }

    async fn rpc_call<T: serde::de::DeserializeOwned, P: Serialize>(
        &self,
        method: &'static str,
        params: P,
    ) -> AppResult<T> {
        let settings = self.rpc_settings.read().await;
        let client = Self::create_http_client(&settings.rpc_proxy)?;

        let request = JsonRpcRequest {
            jsonrpc: "1.0",
            id: 1,
            method,
            params,
        };

        let mut request_builder = client.post(&settings.primary_rpc);

        // Add basic auth if configured
        if let (Some(user), Some(pass)) = (&settings.rpc_user, &settings.rpc_password) {
            request_builder = request_builder.basic_auth(user, Some(pass));
        }

        let response = request_builder
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError::BlockchainError(format!("RPC request failed: {}", e)))?;

        let rpc_response: JsonRpcResponse<T> = response
            .json()
            .await
            .map_err(|e| AppError::BlockchainError(format!("Failed to parse RPC response: {}", e)))?;

        if let Some(error) = rpc_response.error {
            return Err(AppError::BlockchainError(format!(
                "RPC error {}: {}",
                error.code, error.message
            )));
        }

        rpc_response
            .result
            .ok_or_else(|| AppError::BlockchainError("Empty RPC response".to_string()))
    }

    pub fn new(config: &ZcashConfig) -> AppResult<Self> {
        tracing::info!("Initializing Zcash client with RPC: {}", config.rpc_url);

        if config.rpc_proxy.is_some() {
            tracing::info!("Zcash RPC proxy enabled");
        }

        Ok(Self {
            rpc_settings: RwLock::new(RpcSettings {
                primary_rpc: config.rpc_url.clone(),
                fallback_rpcs: config.fallback_rpcs.clone(),
                rpc_proxy: config.rpc_proxy.clone(),
                rpc_user: config.rpc_user.clone(),
                rpc_password: config.rpc_password.clone(),
            }),
            orchard_scanner: RwLock::new(None),
            network_ctx: RwLock::new(None),
        })
    }

    /// Update RPC configuration dynamically (no restart required)
    #[allow(dead_code)]
    pub async fn update_rpc(
        &self,
        primary_rpc: String,
        fallback_rpcs: Option<Vec<String>>,
    ) -> AppResult<()> {
        // Test new RPC endpoint
        let client = Self::create_http_client(&self.rpc_settings.read().await.rpc_proxy)?;

        let test_request = JsonRpcRequest {
            jsonrpc: "1.0",
            id: 1,
            method: "getblockchaininfo",
            params: (),
        };

        let settings = self.rpc_settings.read().await;
        let mut request_builder = client.post(&primary_rpc);

        if let (Some(user), Some(pass)) = (&settings.rpc_user, &settings.rpc_password) {
            request_builder = request_builder.basic_auth(user, Some(pass));
        }
        drop(settings);

        request_builder
            .json(&test_request)
            .send()
            .await
            .map_err(|e| AppError::BlockchainError(format!("Failed to connect to new RPC: {}", e)))?;

        // Update settings
        let mut settings = self.rpc_settings.write().await;
        settings.primary_rpc = primary_rpc.clone();
        if let Some(fallbacks) = fallback_rpcs {
            settings.fallback_rpcs = fallbacks;
        }
        drop(settings);

        // Endpoint changed: invalidate the cached network context so the next
        // address derivation / scan re-reads the network from the new node.
        *self.network_ctx.write().await = None;

        tracing::info!("Zcash RPC updated dynamically to: {}", primary_rpc);
        Ok(())
    }

    /// Get current RPC URL
    #[allow(dead_code)]
    pub async fn get_current_rpc(&self) -> String {
        self.rpc_settings.read().await.primary_rpc.clone()
    }

    /// Import address into zcashd as watch-only (for balance tracking)
    /// This should be called when creating or importing a wallet
    pub async fn import_address(&self, address: &str, label: &str) -> AppResult<()> {
        // Import as watch-only (rescan=false for speed, can rescan later if needed)
        let result: Result<serde_json::Value, _> = self
            .rpc_call("importaddress", (address, label, false))
            .await;

        match result {
            Ok(_) => {
                tracing::info!("Successfully imported address {} into zcashd wallet", address);
                Ok(())
            }
            Err(e) => {
                // If address already exists, that's fine
                let err_msg = e.to_string();
                if err_msg.contains("already exists") || err_msg.contains("already have") {
                    tracing::debug!("Address {} already in zcashd wallet", address);
                    Ok(())
                } else {
                    tracing::warn!("Failed to import address {} into zcashd: {}", address, e);
                    // Don't fail the operation, just warn
                    Ok(())
                }
            }
        }
    }

    /// Get balance for a specific address using local RPC node
    async fn get_address_balance(&self, address: &str) -> AppResult<Decimal> {
        // Try multiple methods to get balance (prioritize local node)

        // Method 1: Try getaddressbalance RPC (requires addressindex=1 in zcash.conf)
        if let Ok(balance) = self.get_balance_from_addressindex(address).await {
            tracing::debug!("Got balance via getaddressbalance RPC: {}", balance);
            return Ok(balance);
        }

        // Method 2: Try z_getbalance (works for addresses in wallet)
        if let Ok(balance) = self.get_balance_from_z_getbalance(address).await {
            tracing::debug!("Got balance via z_getbalance RPC: {}", balance);
            return Ok(balance);
        }

        // Method 3: Try listunspent (only works if address is in wallet)
        let unspent: Vec<ListUnspentEntry> = self
            .rpc_call("listunspent", (0, 9999999, vec![address]))
            .await
            .unwrap_or_else(|_| Vec::new());

        if !unspent.is_empty() {
            let total: f64 = unspent
                .iter()
                .filter(|u| u.address == address && u.confirmations > 0)
                .map(|u| u.amount)
                .sum();

            tracing::debug!("Got balance via listunspent RPC: {}", total);
            return Ok(Decimal::from_str(&format!("{:.8}", total))
                .unwrap_or_else(|_| Decimal::ZERO));
        }

        // If all local methods fail, return 0 with warning
        tracing::warn!(
            "Could not get balance for address {} - ensure addressindex=1 is set in zcash.conf or import the address",
            address
        );
        Ok(Decimal::ZERO)
    }

    /// Get balance using getaddressbalance RPC (Zebra / zcashd with addressindex)
    async fn get_balance_from_addressindex(&self, address: &str) -> AppResult<Decimal> {
        #[derive(Debug, Deserialize)]
        struct AddressBalanceResult {
            balance: i64,
            #[allow(dead_code)]
            received: i64,
        }

        // JSON-RPC params must be an array: [{"addresses": ["t1..."]}]
        let result: AddressBalanceResult = self
            .rpc_call("getaddressbalance", (serde_json::json!({"addresses": [address]}),))
            .await?;

        // Balance is in zatoshis (1 ZEC = 100000000 zatoshis)
        let balance_zec = result.balance as f64 / 100_000_000.0;

        Ok(Decimal::from_str(&format!("{:.8}", balance_zec))
            .unwrap_or_else(|_| Decimal::ZERO))
    }

    /// Get balance using z_getbalance RPC (works for addresses in wallet)
    async fn get_balance_from_z_getbalance(&self, address: &str) -> AppResult<Decimal> {
        let balance: f64 = self
            .rpc_call("z_getbalance", (address,))
            .await?;

        Ok(Decimal::from_str(&format!("{:.8}", balance))
            .unwrap_or_else(|_| Decimal::ZERO))
    }

    /// Get UTXOs for an address using getaddressutxos RPC (Zebra compatible)
    async fn get_address_utxos(&self, address: &str) -> AppResult<Vec<AddressUtxo>> {
        let utxos: Vec<AddressUtxo> = self
            .rpc_call("getaddressutxos", (serde_json::json!({"addresses": [address]}),))
            .await?;

        tracing::debug!("Found {} UTXOs for address {}", utxos.len(), address);
        Ok(utxos)
    }

    /// Send shielded transaction using z_sendmany RPC
    /// This is the proper way to send privacy transactions via zcashd
    ///
    /// # Arguments
    /// * `from_address` - Source address (transparent t1.., unified u1.., or shielded zs..)
    /// * `to_address` - Destination address
    /// * `amount_zec` - Amount in ZEC
    /// * `memo` - Optional encrypted memo (for shielded recipients)
    ///
    /// # Returns
    /// * Transaction ID on success
    pub async fn z_sendmany(
        &self,
        from_address: &str,
        to_address: &str,
        amount_zec: f64,
        memo: Option<String>,
    ) -> AppResult<String> {
        // Build recipient list
        let recipients = vec![ZSendManyRecipient {
            address: to_address.to_string(),
            amount: amount_zec,
            memo: memo.map(|m| hex::encode(m.as_bytes())), // Memo must be hex-encoded
        }];

        // Call z_sendmany with:
        // - from_address
        // - recipients array
        // - minconf (1 = require 1 confirmation)
        // - fee (null = use default)
        // - privacyPolicy ("AllowRevealedSenders" allows transparent -> shielded)
        let opid: String = self
            .rpc_call(
                "z_sendmany",
                (
                    from_address,
                    &recipients,
                    1,                              // minconf
                    serde_json::Value::Null,        // fee (default)
                    "AllowRevealedSenders",         // privacy policy for t->z transfers
                ),
            )
            .await
            .map_err(|e| {
                tracing::error!("z_sendmany RPC failed: {}", e);
                e
            })?;

        tracing::info!("z_sendmany operation started: {}", opid);

        // Wait for operation to complete
        self.wait_for_operation(&opid).await
    }

    /// Wait for a z_* operation to complete and return the txid
    async fn wait_for_operation(&self, opid: &str) -> AppResult<String> {
        let max_attempts = 120; // Wait up to 2 minutes
        let poll_interval = std::time::Duration::from_secs(1);

        for attempt in 0..max_attempts {
            let statuses: Vec<ZOperationStatus> = self
                .rpc_call("z_getoperationstatus", (vec![opid],))
                .await?;

            if let Some(status) = statuses.first() {
                match status.status.as_str() {
                    "success" => {
                        if let Some(ref result) = status.result {
                            tracing::info!(
                                "z_sendmany operation {} succeeded: txid={}",
                                opid,
                                result.txid
                            );
                            return Ok(result.txid.clone());
                        } else {
                            return Err(AppError::BlockchainError(
                                "Operation succeeded but no txid returned".to_string(),
                            ));
                        }
                    }
                    "failed" => {
                        let error_msg = status
                            .error
                            .as_ref()
                            .map(|e| e.message.clone())
                            .unwrap_or_else(|| "Unknown error".to_string());
                        tracing::error!("z_sendmany operation {} failed: {}", opid, error_msg);
                        return Err(AppError::BlockchainError(format!(
                            "Shielded transfer failed: {}",
                            error_msg
                        )));
                    }
                    "executing" | "queued" => {
                        tracing::debug!(
                            "z_sendmany operation {} status: {} (attempt {}/{})",
                            opid,
                            status.status,
                            attempt + 1,
                            max_attempts
                        );
                    }
                    other => {
                        tracing::warn!("Unknown operation status: {}", other);
                    }
                }
            }

            tokio::time::sleep(poll_interval).await;
        }

        Err(AppError::BlockchainError(format!(
            "Timeout waiting for operation {} to complete",
            opid
        )))
    }

    /// Get current block height using getblockcount
    async fn get_block_count(&self) -> AppResult<u64> {
        // Try getblockcount first
        if let Ok(count) = self.rpc_call::<u64, _>("getblockcount", ()).await {
            return Ok(count);
        }

        // Fallback to getblockchaininfo
        let info = self.get_blockchain_info().await?;
        Ok(info.blocks)
    }

    /// Get blockchain info including block height and consensus branch ID
    async fn get_blockchain_info(&self) -> AppResult<BlockchainInfo> {
        // Use empty array for params, not ()
        let empty_params: [(); 0] = [];
        let info: BlockchainInfo = self
            .rpc_call("getblockchaininfo", empty_params)
            .await?;
        Ok(info)
    }

    /// Resolve (and cache) the consensus network context from the live node.
    ///
    /// The network moniker and NU5 activation height are read from
    /// getblockchaininfo so address HRPs and scan start heights follow whatever
    /// chain the RPC endpoint points at (mainnet / testnet / regtest). Cached
    /// per endpoint; `update_rpc` clears it. Falls back to the mainnet NU5
    /// constant only if the node omits the upgrades table.
    async fn resolve_network_ctx(&self) -> AppResult<NetworkContext> {
        if let Some(ctx) = *self.network_ctx.read().await {
            return Ok(ctx);
        }

        let info = self.get_blockchain_info().await?;
        let network = info.network_type();
        let orchard_activation_height = info.nu5_activation_height().unwrap_or_else(|| {
            let fallback = match network {
                NetworkType::Test => 1_842_420,
                _ => 1_687_104,
            };
            tracing::warn!(
                "getblockchaininfo has no NU5 upgrade entry; falling back to {} for {:?}",
                fallback,
                network
            );
            fallback
        });

        let nu63_activation_height = info.nu63_activation_height();
        let ctx = NetworkContext {
            network,
            orchard_activation_height,
            nu63_activation_height,
        };
        *self.network_ctx.write().await = Some(ctx);
        tracing::info!(
            "Resolved Zcash network context: network={:?}, orchard_activation_height={}, nu63_activation_height={:?}",
            network,
            orchard_activation_height,
            nu63_activation_height
        );
        Ok(ctx)
    }

    /// Send raw transaction via sendrawtransaction RPC (Zebra compatible)
    async fn send_raw_transaction(&self, raw_tx_hex: &str) -> AppResult<String> {
        tracing::info!(
            "[ZEC RPC] sendrawtransaction: tx_hex_len={}, tx_hex_prefix={}...",
            raw_tx_hex.len(),
            &raw_tx_hex[..std::cmp::min(64, raw_tx_hex.len())]
        );

        let result: Result<String, _> = self
            .rpc_call("sendrawtransaction", (raw_tx_hex,))
            .await;

        match &result {
            Ok(tx_hash) => {
                tracing::info!("[ZEC RPC] sendrawtransaction SUCCESS: tx_hash={}", tx_hash);
            }
            Err(e) => {
                tracing::error!(
                    "[ZEC RPC] sendrawtransaction FAILED: error={}\n  tx_hex_len={}\n  tx_hex_first_128={}",
                    e,
                    raw_tx_hex.len(),
                    &raw_tx_hex[..std::cmp::min(128, raw_tx_hex.len())]
                );
            }
        }

        result
    }

    /// Send ZEC by building and signing a raw transaction (Zebra compatible)
    /// This replaces the old sendtoaddress approach which is not supported by Zebra
    async fn send_zec(
        &self,
        from_address: &str,
        to_address: &str,
        amount: Decimal,
        private_key: &str,
    ) -> AppResult<String> {
        use crate::blockchain::zcash::transaction::{build_and_sign_transaction, TransactionBuilder};

        // Convert amount to zatoshis (1 ZEC = 100,000,000 zatoshis)
        let amount_zec_str = amount.to_string();
        let amount_f64: f64 = amount_zec_str
            .parse()
            .map_err(|_| AppError::ValidationError("Invalid amount".to_string()))?;
        let amount_zatoshis = (amount_f64 * 100_000_000.0) as u64;

        // Get UTXOs for the from address
        let utxos = self.get_address_utxos(from_address).await?;
        if utxos.is_empty() {
            return Err(AppError::ValidationError(format!(
                "No UTXOs found for address {}. Cannot send transaction.",
                from_address
            )));
        }

        // Calculate total available balance
        let total_available: u64 = utxos.iter().map(|u| u.satoshis).sum();
        tracing::info!(
            "Found {} UTXOs with total {} zatoshis for {}",
            utxos.len(),
            total_available,
            from_address
        );

        // Fee estimation: ~10000 zatoshis (0.0001 ZEC) for a typical transaction
        // Zcash recommends at least 1000 zatoshis per kB, typical tx is ~250 bytes
        let fee_zatoshis: u64 = 10000;

        let total_needed = amount_zatoshis + fee_zatoshis;
        if total_available < total_needed {
            return Err(AppError::ValidationError(format!(
                "Insufficient balance: have {} zatoshis, need {} zatoshis (amount {} + fee {})",
                total_available, total_needed, amount_zatoshis, fee_zatoshis
            )));
        }

        // Get current blockchain info for height and consensus branch ID
        let blockchain_info = self.get_blockchain_info().await?;
        let current_height = blockchain_info.blocks;
        let expiry_height = current_height + 40; // Expire in ~40 blocks (~1 hour)

        // Parse consensus branch ID from hex string (e.g., "c8e71055" -> 0xc8e71055)
        let consensus_branch_id = u32::from_str_radix(&blockchain_info.consensus.chaintip, 16)
            .map_err(|e| AppError::BlockchainError(format!(
                "Failed to parse consensus branch ID '{}': {}",
                blockchain_info.consensus.chaintip, e
            )))?;

        tracing::info!(
            "Building transaction at height {}, expiry height {}, consensus branch ID: 0x{:08x}",
            current_height,
            expiry_height,
            consensus_branch_id
        );

        // Build transaction with consensus branch ID from node
        let mut builder = TransactionBuilder::new_with_branch_id(
            expiry_height as u32,
            consensus_branch_id,
        );

        // Select UTXOs (simple: use all of them, or just enough)
        let mut input_total: u64 = 0;
        for utxo in &utxos {
            let txid_bytes = hex::decode(&utxo.txid)
                .map_err(|e| AppError::ValidationError(format!("Invalid txid hex: {}", e)))?;
            if txid_bytes.len() != 32 {
                return Err(AppError::ValidationError(format!(
                    "Invalid txid length: expected 32 bytes, got {}",
                    txid_bytes.len()
                )));
            }

            let script_bytes = hex::decode(&utxo.script)
                .map_err(|e| AppError::ValidationError(format!("Invalid script hex: {}", e)))?;

            builder.add_input(
                txid_bytes.try_into().unwrap(),
                utxo.output_index,
                utxo.satoshis,
                script_bytes,
            );
            input_total += utxo.satoshis;

            // If we have enough, stop adding inputs
            if input_total >= total_needed {
                break;
            }
        }

        // Add output to recipient
        builder.add_output(to_address, amount_zatoshis)?;

        // Add change output if needed
        let change = input_total - amount_zatoshis - fee_zatoshis;
        if change > 0 {
            // Dust threshold is typically 546 satoshis for Bitcoin, similar for Zcash
            if change > 546 {
                builder.add_output(from_address, change)?;
            } else {
                // Add dust to fee
                tracing::debug!("Change {} zatoshis below dust threshold, adding to fee", change);
            }
        }

        // Build and sign the transaction
        let raw_tx_hex = build_and_sign_transaction(&builder, private_key)?;

        tracing::info!(
            "Built raw transaction: {} bytes",
            raw_tx_hex.len() / 2
        );

        // Send the raw transaction
        let tx_hash = self.send_raw_transaction(&raw_tx_hex).await?;

        tracing::info!("ZEC transfer submitted via sendrawtransaction: {}", tx_hash);
        Ok(tx_hash)
    }

    // =========================================================================
    // Orchard Privacy Protocol Methods
    // =========================================================================

    /// Initialize Orchard scanner with viewing keys
    ///
    /// Call this when enabling Orchard for an account
    pub async fn init_orchard_scanner(&self, viewing_keys: Vec<OrchardViewingKey>) -> AppResult<()> {
        let scanner = OrchardScanner::new(viewing_keys);
        let mut scanner_lock = self.orchard_scanner.write().await;
        *scanner_lock = Some(scanner);

        tracing::info!("Orchard scanner initialized");
        Ok(())
    }

    /// Add a viewing key to the scanner
    pub async fn add_orchard_viewing_key(&self, viewing_key: OrchardViewingKey) -> AppResult<()> {
        let mut scanner_lock = self.orchard_scanner.write().await;

        if let Some(ref mut _scanner) = *scanner_lock {
            // Create a new scanner with the additional key
            // In production, this should be more efficient
            drop(scanner_lock);
            let current_keys = Vec::new(); // Would need to track keys
            let mut new_keys = current_keys;
            new_keys.push(viewing_key);
            self.init_orchard_scanner(new_keys).await?;
        } else {
            *scanner_lock = Some(OrchardScanner::new(vec![viewing_key]));
        }

        Ok(())
    }

    /// Get shielded balance for a wallet
    pub async fn get_orchard_balance(&self, wallet_id: i32) -> AppResult<ShieldedBalance> {
        let scanner_lock = self.orchard_scanner.read().await;

        let scanner = scanner_lock.as_ref().ok_or_else(|| {
            AppError::BlockchainError("Orchard scanner not initialized".to_string())
        })?;

        let current_height = self.get_block_count().await?;
        let total = scanner.get_balance(wallet_id);
        let spendable = scanner.get_spendable_balance(wallet_id, current_height);
        let notes = scanner.get_unspent_notes(wallet_id);

        Ok(ShieldedBalance::new(
            ShieldedPool::Orchard,
            total,
            spendable,
            notes.len() as u32,
        ))
    }

    /// Get scan progress
    pub async fn get_scan_progress(&self) -> AppResult<ScanProgress> {
        let scanner_lock = self.orchard_scanner.read().await;

        let scanner = scanner_lock.as_ref().ok_or_else(|| {
            AppError::BlockchainError("Orchard scanner not initialized".to_string())
        })?;

        Ok(scanner.progress().clone())
    }

    /// Build an Orchard (shielded) transaction
    ///
    /// This prepares a shielded transaction but does not sign or broadcast it.
    /// The transaction will spend from the Orchard pool and output to the specified address.
    pub async fn build_orchard_transaction(
        &self,
        params: &OrchardTransferParams,
        private_key_hex: &str,
    ) -> AppResult<Vec<u8>> {
        // Get spendable notes
        let scanner_lock = self.orchard_scanner.read().await;
        let scanner = scanner_lock.as_ref().ok_or_else(|| {
            AppError::BlockchainError("Orchard scanner not initialized".to_string())
        })?;

        let current_height = self.get_block_count().await?;
        let spendable_notes = scanner.get_spendable_notes(params.wallet_id, current_height);

        if spendable_notes.is_empty() {
            return Err(AppError::ValidationError(
                "No spendable Orchard notes found".to_string(),
            ));
        }

        // Get anchor from scanner
        let anchor = scanner.get_anchor();
        let anchor_height = current_height.saturating_sub(10);
        drop(scanner_lock);

        // Get blockchain info
        let blockchain_info = self.get_blockchain_info().await?;
        let expiry_height = (current_height + 40) as u32;
        let consensus_branch_id = u32::from_str_radix(&blockchain_info.consensus.chaintip, 16)
            .map_err(|e| AppError::BlockchainError(format!("Invalid branch ID: {}", e)))?;

        // Derive spending key (account_index is always 0 since each wallet has its own private key)
        let (spending_key, _) = OrchardKeyManager::derive_from_private_key(private_key_hex, 0, 0)
            .map_err(|e| AppError::InternalError(format!("Failed to derive spending key: {}", e)))?;

        // Build transaction
        let mut builder = OrchardTransactionBuilder::new(
            anchor_height,
            anchor,
            expiry_height,
            consensus_branch_id,
        );

        // Add spendable notes
        builder.add_spendable_notes(spendable_notes);

        // Add output
        builder
            .add_output(
                &params.to_address,
                params.amount_zatoshis,
                params.memo.as_deref(),
            )
            .map_err(|e| AppError::BlockchainError(format!("Failed to add output: {}", e)))?;

        // Build the bundle
        let bundle = builder
            .build(&spending_key, params)
            .map_err(|e| AppError::BlockchainError(format!("Failed to build transaction: {}", e)))?;

        // Serialize for transmission
        let raw_tx = bundle.serialize();

        tracing::info!(
            "Built Orchard transaction with {} actions, {} bytes",
            bundle.num_actions(),
            raw_tx.len()
        );

        Ok(raw_tx)
    }

    /// Execute an Orchard shielded transfer
    pub async fn transfer_orchard(
        &self,
        params: &OrchardTransferParams,
        private_key_hex: &str,
    ) -> AppResult<String> {
        // Build the transaction
        let raw_tx = self.build_orchard_transaction(params, private_key_hex).await?;

        // Broadcast
        let tx_hash = self.send_raw_transaction(&hex::encode(&raw_tx)).await?;

        // Mark spent notes
        // (In production, this would be done after confirmation)

        tracing::info!("Orchard transfer submitted: {}", tx_hash);
        Ok(tx_hash)
    }

    /// Get Orchard transaction history for a wallet
    pub async fn get_orchard_transactions(
        &self,
        wallet_id: i32,
        limit: u32,
    ) -> AppResult<Vec<OrchardTransactionInfo>> {
        let scanner_lock = self.orchard_scanner.read().await;
        let scanner = scanner_lock.as_ref().ok_or_else(|| {
            AppError::BlockchainError("Orchard scanner not initialized".to_string())
        })?;

        let notes = scanner.get_unspent_notes(wallet_id);

        // Convert notes to transaction info
        let transactions: Vec<OrchardTransactionInfo> = notes
            .into_iter()
            .take(limit as usize)
            .map(|note| OrchardTransactionInfo {
                tx_hash: note.tx_hash,
                block_height: note.block_height,
                value_zatoshis: note.value_zatoshis,
                is_incoming: true,
                memo: note.memo,
                pool: ShieldedPool::Orchard,
            })
            .collect();

        Ok(transactions)
    }

    /// Sync Orchard scanner with the blockchain
    ///
    /// This should be called periodically to detect new notes
    pub async fn sync_orchard(&self) -> AppResult<ScanProgress> {
        // Get current chain tip
        let chain_tip = self.get_block_count().await?;

        let mut scanner_lock = self.orchard_scanner.write().await;
        let scanner = scanner_lock.as_mut().ok_or_else(|| {
            AppError::BlockchainError("Orchard scanner not initialized".to_string())
        })?;

        // Get last scanned height
        let last_height = scanner.progress().last_scanned_height;

        if last_height >= chain_tip {
            return Ok(scanner.progress().clone());
        }

        // Fetch compact blocks (in production, use lightwalletd)
        // For now, just update progress
        tracing::info!(
            "Would scan blocks {} to {}",
            last_height + 1,
            chain_tip
        );

        Ok(scanner.progress().clone())
    }
}

/// Information about an Orchard transaction
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrchardTransactionInfo {
    pub tx_hash: String,
    pub block_height: u64,
    pub value_zatoshis: u64,
    pub is_incoming: bool,
    pub memo: Option<String>,
    pub pool: ShieldedPool,
}

#[async_trait]
impl ChainClient for ZcashClient {
    fn chain_id(&self) -> &str {
        "zcash"
    }

    fn chain_name(&self) -> &str {
        "Zcash Mainnet"
    }

    fn native_token_symbol(&self) -> &str {
        "ZEC"
    }

    async fn get_native_balance(&self, address: &str) -> AppResult<Decimal> {
        let start = std::time::Instant::now();
        tracing::debug!("Getting ZEC balance for {}", address);

        let balance = self.get_address_balance(address).await?;

        tracing::debug!(
            "ZEC balance for {}: {} (took {}ms)",
            address,
            balance,
            start.elapsed().as_millis()
        );

        Ok(balance)
    }

    async fn get_token_balance(&self, _address: &str, token_symbol: &str) -> AppResult<Decimal> {
        // Zcash doesn't have ERC20-style tokens
        // Only native ZEC is supported
        Err(AppError::NotFound(format!(
            "Token {} not supported on Zcash. Only native ZEC is available.",
            token_symbol
        )))
    }

    async fn get_all_balances(&self, address: &str) -> AppResult<(Decimal, Vec<TokenBalance>)> {
        let native_balance = self.get_native_balance(address).await?;
        // Zcash only has native token, no additional tokens
        Ok((native_balance, Vec::new()))
    }

    async fn estimate_gas(&self, _params: &TransferParams) -> AppResult<GasEstimate> {
        // Zcash uses transaction fees, not gas
        // Estimate fee using estimatefee RPC
        let fee_per_kb: f64 = self
            .rpc_call::<EstimateFeeResult, _>("estimatefee", (6,))
            .await
            .map(|r| r.0)
            .unwrap_or(0.0001); // Default fee if estimation fails

        // Typical Zcash transparent transaction is ~250 bytes
        // For shielded transactions it can be much larger
        let estimated_tx_size_kb = 0.25;
        let estimated_fee = fee_per_kb * estimated_tx_size_kb;

        // Ensure minimum fee
        let min_fee = 0.00001;
        let fee = if estimated_fee < min_fee {
            min_fee
        } else {
            estimated_fee
        };

        let fee_decimal = Decimal::from_str(&format!("{:.8}", fee)).unwrap_or_else(|_| Decimal::ZERO);

        // For Zcash, we use fee terminology instead of gas
        // But we map to the same structure for API consistency
        Ok(GasEstimate {
            gas_limit: 1, // Not applicable for Zcash, use 1 as placeholder
            gas_price_gwei: fee_decimal, // Use this field to represent fee in ZEC
            estimated_fee_eth: fee_decimal, // Fee in ZEC
            base_fee_gwei: None,
            priority_fee_gwei: None,
            max_fee_gwei: Some(fee_decimal),
        })
    }

    async fn transfer_native(&self, params: &TransferParams) -> AppResult<String> {
        let tx_hash = self
            .send_zec(
                &params.from_address,
                &params.to_address,
                params.amount,
                &params.private_key,
            )
            .await?;

        tracing::info!("ZEC transfer submitted: {}", tx_hash);
        Ok(tx_hash)
    }

    async fn transfer_token(&self, params: &TransferParams) -> AppResult<String> {
        // Zcash doesn't support tokens like ERC20
        Err(AppError::NotFound(format!(
            "Token {} not supported on Zcash. Only native ZEC transfers are available.",
            params.token
        )))
    }

    async fn get_tx_status(&self, tx_hash: &str) -> AppResult<TxStatus> {
        let tx_result: Result<GetTransactionResult, _> =
            self.rpc_call("gettransaction", (tx_hash,)).await;

        match tx_result {
            Ok(tx) => {
                let confirmations = tx.confirmations.unwrap_or(0);
                if confirmations >= 1 {
                    // Get block height
                    let block_number = if let Some(blockhash) = &tx.blockhash {
                        let block: GetBlockResult = self
                            .rpc_call("getblock", (blockhash,))
                            .await
                            .unwrap_or(GetBlockResult { height: 0, time: None });
                        block.height
                    } else {
                        0
                    };

                    Ok(TxStatus::Confirmed {
                        block_number,
                        gas_used: 0, // Zcash doesn't have gas concept
                    })
                } else if confirmations == 0 {
                    Ok(TxStatus::Pending)
                } else {
                    // Negative confirmations means the transaction was invalidated
                    Ok(TxStatus::Failed {
                        reason: "Transaction was invalidated or orphaned".to_string(),
                    })
                }
            }
            Err(_) => Ok(TxStatus::NotFound),
        }
    }

    fn validate_address(&self, address: &str) -> bool {
        // Zcash transparent addresses start with 't1' or 't3' (mainnet)
        // Shielded addresses start with 'zs' (Sapling) or 'zc' (Sprout)
        // Unified addresses start with 'u1'
        if address.is_empty() {
            return false;
        }

        // Basic format validation for transparent addresses
        let is_transparent = (address.starts_with("t1") || address.starts_with("t3"))
            && address.len() >= 34
            && address.len() <= 36;

        // Basic format validation for shielded addresses
        let is_sapling = address.starts_with("zs") && address.len() >= 78;
        let is_sprout = address.starts_with("zc") && address.len() >= 95;

        // Basic format validation for unified addresses
        let is_unified = address.starts_with("u1") && address.len() >= 100;

        is_transparent || is_sapling || is_sprout || is_unified
    }

    async fn get_gas_price(&self) -> AppResult<Decimal> {
        // Return estimated fee per KB for Zcash
        let fee_per_kb: f64 = self
            .rpc_call::<EstimateFeeResult, _>("estimatefee", (6,))
            .await
            .map(|r| r.0)
            .unwrap_or(0.0001);

        Ok(Decimal::from_str(&format!("{:.8}", fee_per_kb)).unwrap_or_else(|_| Decimal::ZERO))
    }

    async fn import_address_for_tracking(&self, address: &str, label: &str) -> AppResult<()> {
        self.import_address(address, label).await
    }

    async fn get_block_height(&self) -> AppResult<u64> {
        self.get_block_count().await
    }

    /// Binary-search the chain for the lowest block whose `time >= timestamp`.
    /// Anchor at the chain tip so we never wander past it.  Costs ~log2(tip)
    /// RPC round-trips — at mainnet tip 3.3M that is ~22 `getblock` calls,
    /// well inside the disclosure async-job budget.
    async fn block_at_timestamp(&self, timestamp: i64) -> AppResult<u64> {
        // Helper: fetch the block's unix timestamp.  `getblock <height>` with
        // default verbosity (1) returns an object containing `time`.
        async fn block_time(client: &ZcashClient, height: u64) -> AppResult<i64> {
            let block: GetBlockResult = client
                .rpc_call("getblock", (height.to_string(),))
                .await?;
            block.time.ok_or_else(|| {
                AppError::BlockchainError(format!(
                    "block {} has no time field in RPC response",
                    height
                ))
            })
        }

        let tip = self.get_block_count().await?;
        if tip == 0 {
            return Err(AppError::BlockchainError(
                "chain tip is 0 — cannot resolve timestamp".to_string(),
            ));
        }

        // If the request is past the tip, return the tip — disclosure
        // `to=now` is the common case and should not error.
        let tip_time = block_time(self, tip).await?;
        if timestamp >= tip_time {
            return Ok(tip);
        }

        // Genesis check — if the request is before the chain even started,
        // anchor at block 1.  Block 0 is technically valid but `getblock 0`
        // behaves oddly across zcashd / zebra; 1 is the safe floor here.
        let mut low: u64 = 1;
        let mut high: u64 = tip;
        let low_time = block_time(self, low).await?;
        if timestamp <= low_time {
            return Ok(low);
        }

        // Standard binary search for the lowest height whose time >= ts.
        while low < high {
            let mid = low + (high - low) / 2;
            let t = block_time(self, mid).await?;
            if t < timestamp {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        Ok(low)
    }

    async fn broadcast_raw_transaction(&self, raw_tx_hex: &str) -> AppResult<String> {
        self.send_raw_transaction(raw_tx_hex).await
    }

    async fn get_utxos(&self, address: &str) -> AppResult<Vec<Utxo>> {
        let utxos = self.get_address_utxos(address).await?;
        Ok(utxos
            .into_iter()
            .map(|u| Utxo {
                txid: u.txid,
                output_index: u.output_index,
                script: u.script,
                value: u.satoshis,
                height: u.height,
            })
            .collect())
    }

    async fn send_shielded(
        &self,
        from_address: &str,
        to_address: &str,
        amount: Decimal,
        memo: Option<String>,
    ) -> AppResult<String> {
        let amount_f64: f64 = amount
            .to_string()
            .parse()
            .map_err(|_| AppError::ValidationError("Invalid amount".to_string()))?;

        self.z_sendmany(from_address, to_address, amount_f64, memo).await
    }

    async fn get_rpc_url(&self) -> Option<String> {
        Some(self.rpc_settings.read().await.primary_rpc.clone())
    }

    async fn get_rpc_auth(&self) -> Option<(String, String)> {
        let settings = self.rpc_settings.read().await;
        match (&settings.rpc_user, &settings.rpc_password) {
            (Some(user), Some(pass)) => Some((user.clone(), pass.clone())),
            _ => None,
        }
    }

    async fn get_shielded_network(&self) -> AppResult<String> {
        let ctx = self.resolve_network_ctx().await?;
        let moniker = match ctx.network {
            NetworkType::Main => "main",
            NetworkType::Test => "test",
            NetworkType::Regtest => "regtest",
        };
        Ok(moniker.to_string())
    }

    async fn get_shielded_activation_height(&self) -> AppResult<u64> {
        Ok(self.resolve_network_ctx().await?.orchard_activation_height)
    }

    async fn get_consensus_branch_id(&self) -> AppResult<u32> {
        // Read fresh (not cached): the active branch id changes across upgrades
        // and must reflect the current chain tip for the broadcast to be valid.
        let info = self.get_blockchain_info().await?;
        u32::from_str_radix(&info.consensus.chaintip, 16).map_err(|e| {
            AppError::BlockchainError(format!(
                "Failed to parse consensus branch id '{}': {}",
                info.consensus.chaintip, e
            ))
        })
    }

    async fn get_nu63_activation_height(&self) -> AppResult<Option<u64>> {
        Ok(self.resolve_network_ctx().await?.nu63_activation_height)
    }
}
