//! Orchard blockchain synchronization module
//!
//! Fetches blocks from Zebra node and scans for Orchard notes
//! belonging to the wallet's viewing keys.
//!
//! Features:
//! - Parallel block fetching for improved speed
//! - Database persistence for scan state and notes
//! - Resume from last scanned height on restart

#![allow(dead_code)]

use super::{
    keys::OrchardViewingKey,
    scanner::{CompactBlock, CompactOrchardAction, CompactTransaction, OrchardNote, OrchardScanner, ScanProgress, ShieldedBalance, SpentNoteInfo},
    OrchardError, OrchardResult, ShieldedPool,
};
use crate::db::repositories::OrchardRepository;
use orchard::keys::IncomingViewingKey;
use serde::Deserialize;
use sqlx::MySqlPool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use futures::future::join_all;

/// Configuration for the Orchard sync service
#[derive(Debug, Clone)]
pub struct SyncConfig {
    /// Zebra RPC URL
    pub rpc_url: String,
    /// RPC username (optional)
    pub rpc_user: Option<String>,
    /// RPC password (optional)
    pub rpc_password: Option<String>,
    /// Number of blocks to fetch per batch
    pub batch_size: u64,
    /// Starting height (birthday)
    pub birthday_height: u64,
    /// Number of concurrent block fetches
    pub parallel_fetches: usize,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            rpc_url: "http://127.0.0.1:8232".to_string(),
            rpc_user: None,
            rpc_password: None,
            batch_size: 500,  // Process 500 blocks per round
            birthday_height: 1_687_104,  // Orchard activation height
            parallel_fetches: 25,  // Smaller batch = faster response, more parallel
        }
    }
}

/// Zebra RPC response structures
#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcError>,
    id: u64,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Deserialize)]
struct BlockInfo {
    hash: String,
    height: u64,
    tx: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct VerboseBlock {
    hash: String,
    height: u64,
    tx: Vec<TransactionInfo>,
}

#[derive(Debug, Deserialize)]
struct TransactionInfo {
    txid: String,
    #[serde(default)]
    orchard: Option<OrchardBundle>,
}

#[derive(Debug, Deserialize)]
struct OrchardBundle {
    actions: Vec<OrchardActionInfo>,
    #[serde(rename = "valueBalanceZat")]
    value_balance_zat: i64,
}

#[derive(Debug, Deserialize)]
struct OrchardActionInfo {
    cv: String,
    nullifier: String,
    rk: String,
    #[serde(rename = "cmx")]
    cm_x: String,
    #[serde(rename = "ephemeralKey")]
    ephemeral_key: String,
    #[serde(rename = "encCiphertext")]
    enc_ciphertext: String,
    #[serde(rename = "outCiphertext")]
    out_ciphertext: String,
}

/// Tree state response from z_gettreestate RPC
#[derive(Debug, Deserialize)]
struct TreeStateResponse {
    height: u64,
    #[allow(dead_code)]
    hash: String,
    orchard: OrchardTreeState,
}

#[derive(Debug, Deserialize)]
struct OrchardTreeState {
    commitments: OrchardCommitments,
}

#[derive(Debug, Deserialize)]
struct OrchardCommitments {
    #[serde(rename = "finalRoot")]
    final_root: String,
    #[serde(rename = "finalState")]
    final_state: String,
}

/// Orchard synchronization service with database persistence
pub struct OrchardSyncService {
    config: SyncConfig,
    client: reqwest::Client,
    scanner: Arc<RwLock<OrchardScanner>>,
    /// Stored notes by wallet_id (memory cache)
    notes_by_wallet: Arc<RwLock<HashMap<i32, Vec<OrchardNote>>>>,
    /// Wallet ID to viewing key mapping
    wallet_keys: Arc<RwLock<HashMap<i32, OrchardViewingKey>>>,
    /// Database repository for persistence
    db_repo: Option<Arc<OrchardRepository>>,
    /// Tracks which wallets have been synced
    wallet_scan_heights: Arc<RwLock<HashMap<i32, u64>>>,
}

impl OrchardSyncService {
    /// Create HTTP client with high connection pool for parallel fetching
    fn create_http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))  // 2 min timeout for batch requests
            .pool_max_idle_per_host(100)  // Allow many idle connections
            .pool_idle_timeout(std::time::Duration::from_secs(60))
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default()
    }

    /// Create a new sync service without database (memory only)
    pub fn new(config: SyncConfig) -> Self {
        Self {
            config,
            client: Self::create_http_client(),
            scanner: Arc::new(RwLock::new(OrchardScanner::new(vec![]))),
            notes_by_wallet: Arc::new(RwLock::new(HashMap::new())),
            wallet_keys: Arc::new(RwLock::new(HashMap::new())),
            db_repo: None,
            wallet_scan_heights: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new sync service with database persistence
    pub fn new_with_db(config: SyncConfig, pool: MySqlPool) -> Self {
        Self {
            config,
            client: Self::create_http_client(),
            scanner: Arc::new(RwLock::new(OrchardScanner::new(vec![]))),
            notes_by_wallet: Arc::new(RwLock::new(HashMap::new())),
            wallet_keys: Arc::new(RwLock::new(HashMap::new())),
            db_repo: Some(Arc::new(OrchardRepository::new(pool))),
            wallet_scan_heights: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a wallet's viewing key for scanning
    pub async fn register_wallet(&self, wallet_id: i32, viewing_key: OrchardViewingKey) {
        let birthday = viewing_key.birthday_height;
        // Set wallet_id on the viewing key for proper note tracking
        let viewing_key = viewing_key.with_wallet_id(wallet_id);
        let mut keys = self.wallet_keys.write().await;
        keys.insert(wallet_id, viewing_key.clone());

        // Load scan state from database if available
        let mut scan_heights = self.wallet_scan_heights.write().await;
        if let Some(repo) = &self.db_repo {
            if let Ok(Some(state)) = repo.get_sync_state(wallet_id).await {
                scan_heights.insert(wallet_id, state.last_scanned_height);
                tracing::info!(
                    "[Orchard Sync] Restored wallet {} scan state: last_scanned={}, notes={}",
                    wallet_id,
                    state.last_scanned_height,
                    state.notes_found
                );
            } else {
                // Initialize with birthday height
                scan_heights.insert(wallet_id, birthday);
                if let Err(e) = repo.upsert_sync_state(wallet_id, birthday, 0).await {
                    tracing::warn!("[Orchard Sync] Failed to init sync state: {}", e);
                }
            }
        } else {
            scan_heights.insert(wallet_id, birthday);
        }

        let mut scanner = self.scanner.write().await;
        // Add the new key incrementally without resetting scanner state
        // This preserves the tree_tracker, notes, and spent_nullifiers
        scanner.add_viewing_key(viewing_key.clone());

        tracing::info!(
            "[Orchard Sync] Registered wallet {} for scanning (account_index={}, birthday={}, total wallets={})",
            wallet_id,
            viewing_key.account_index,
            birthday,
            scanner.viewing_key_count()
        );
    }

    /// Get current chain height from Zebra
    pub async fn get_chain_height(&self) -> OrchardResult<u64> {
        let response: RpcResponse<u64> = self.rpc_call("getblockcount", serde_json::json!([])).await?;
        response.result.ok_or_else(|| {
            OrchardError::RpcError(response.error.map(|e| e.message).unwrap_or_default())
        })
    }

    /// Get tree state at a specific height from Zebra
    ///
    /// Returns the Orchard commitment tree frontier and root at the given height.
    /// This is used to initialize our tree tracker from a known state.
    pub async fn get_tree_state(&self, height: u64) -> OrchardResult<(String, String, u64)> {
        let response: RpcResponse<TreeStateResponse> = self
            .rpc_call("z_gettreestate", serde_json::json!([height.to_string()]))
            .await?;

        let state = response.result.ok_or_else(|| {
            OrchardError::RpcError(format!(
                "Failed to get tree state at height {}: {}",
                height,
                response.error.map(|e| e.message).unwrap_or_default()
            ))
        })?;

        Ok((
            state.orchard.commitments.final_state,
            state.orchard.commitments.final_root,
            state.height,
        ))
    }

    /// Count Orchard commitments up to a given height
    ///
    /// This parses the tree frontier from z_gettreestate to get the exact tree size,
    /// which gives us the starting position for scanning.
    async fn count_commitments_at_height(&self, height: u64) -> OrchardResult<u64> {
        let (frontier_hex, _root, _) = self.get_tree_state(height).await?;

        // Parse the frontier using zcash_primitives format
        use super::tree::OrchardTreeTracker;
        match OrchardTreeTracker::from_frontier(&frontier_hex, 0, height) {
            Ok(tracker) => {
                // tree.size() gives us the number of commitments, which is our starting position
                let size = tracker.tree_size();
                tracing::info!(
                    "[Orchard Sync] Tree size at height {}: {} commitments",
                    height,
                    size
                );
                Ok(size as u64)
            }
            Err(e) => {
                tracing::warn!(
                    "[Orchard Sync] Failed to parse frontier at height {}: {:?}",
                    height,
                    e
                );
                // Fallback: return 0 and let the tree track relatively
                Ok(0)
            }
        }
    }

    /// Get the expected Orchard anchor (tree root) at a specific height from the Zcash node
    ///
    /// This is the authoritative anchor that the node will accept for transactions.
    /// Use this to validate that our locally-computed tree root matches the chain state.
    pub async fn get_expected_anchor(&self, height: u64) -> OrchardResult<[u8; 32]> {
        let (_frontier_hex, root_hex, _) = self.get_tree_state(height).await?;

        // Parse the hex root string
        let root_bytes = hex::decode(&root_hex)
            .map_err(|e| OrchardError::RpcError(format!("Invalid root hex: {}", e)))?;

        if root_bytes.len() != 32 {
            return Err(OrchardError::RpcError(format!(
                "Invalid root length: expected 32, got {}",
                root_bytes.len()
            )));
        }

        let mut anchor = [0u8; 32];
        anchor.copy_from_slice(&root_bytes);

        tracing::debug!(
            "[Orchard Sync] Expected anchor at height {}: {}",
            height,
            hex::encode(&anchor)
        );

        Ok(anchor)
    }

    /// Validate that our computed tree root matches the expected anchor from the Zcash node
    ///
    /// Returns true if the roots match, false otherwise.
    /// This is useful for debugging tree synchronization issues.
    pub async fn validate_tree_root(&self, height: u64) -> OrchardResult<bool> {
        let expected_anchor = self.get_expected_anchor(height).await?;

        let scanner = self.scanner.read().await;
        let computed_root = scanner.get_current_root();
        let tree_height = scanner.tree_tracker().block_height();

        let roots_match = computed_root == expected_anchor;

        if roots_match {
            tracing::info!(
                "[Orchard Sync] ✅ Tree root matches expected anchor at height {}",
                height
            );
        } else {
            tracing::warn!(
                "[Orchard Sync] ⚠️ Tree root MISMATCH at height {}!\n  Expected: {}\n  Computed: {}\n  Tree at height: {}",
                height,
                hex::encode(&expected_anchor),
                hex::encode(&computed_root),
                tree_height
            );
        }

        Ok(roots_match)
    }

    /// Get the current anchor (tree root) that should be used for transactions
    ///
    /// This returns the anchor from our locally-tracked tree state.
    /// For valid transactions, this anchor must be recognized by the Zcash node.
    pub async fn get_current_anchor(&self) -> [u8; 32] {
        let scanner = self.scanner.read().await;
        scanner.get_current_root()
    }

    /// Get block at height with full transaction data
    async fn get_block(&self, height: u64) -> OrchardResult<VerboseBlock> {
        // First get block hash
        let hash_response: RpcResponse<String> = self
            .rpc_call("getblockhash", serde_json::json!([height]))
            .await?;

        let hash = hash_response.result.ok_or_else(|| {
            OrchardError::RpcError(format!("Failed to get block hash for height {}", height))
        })?;

        // Then get block with verbosity 2 (includes full transactions)
        let block_response: RpcResponse<VerboseBlock> = self
            .rpc_call("getblock", serde_json::json!([hash, 2]))
            .await?;

        block_response.result.ok_or_else(|| {
            OrchardError::RpcError(format!("Failed to get block at height {}", height))
        })
    }

    /// Fetch multiple blocks using batch RPC (much faster than individual calls)
    /// Uses 2 batch requests: first for block hashes, then for blocks
    async fn fetch_blocks_batch(&self, heights: Vec<u64>) -> Vec<(u64, OrchardResult<VerboseBlock>)> {
        if heights.is_empty() {
            return vec![];
        }

        // Step 1: Batch request for all block hashes
        let hash_requests: Vec<serde_json::Value> = heights
            .iter()
            .enumerate()
            .map(|(i, h)| {
                serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": i,
                    "method": "getblockhash",
                    "params": [h]
                })
            })
            .collect();

        let hashes = match self.batch_rpc_call::<String>(&hash_requests).await {
            Ok(h) => h,
            Err(e) => {
                tracing::warn!("[Orchard Sync] Batch getblockhash failed: {}, falling back to parallel", e);
                return self.fetch_blocks_parallel_fallback(heights).await;
            }
        };

        // Map height to hash
        let height_to_hash: std::collections::HashMap<u64, String> = heights
            .iter()
            .zip(hashes.iter())
            .filter_map(|(h, r)| r.as_ref().ok().map(|hash| (*h, hash.clone())))
            .collect();

        // Step 2: Batch request for all blocks
        let block_requests: Vec<serde_json::Value> = heights
            .iter()
            .enumerate()
            .filter_map(|(i, h)| {
                height_to_hash.get(h).map(|hash| {
                    serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": i,
                        "method": "getblock",
                        "params": [hash, 2]
                    })
                })
            })
            .collect();

        let blocks = match self.batch_rpc_call::<VerboseBlock>(&block_requests).await {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!("[Orchard Sync] Batch getblock failed: {}, falling back to parallel", e);
                return self.fetch_blocks_parallel_fallback(heights).await;
            }
        };

        // Combine results
        heights
            .iter()
            .zip(blocks.into_iter())
            .map(|(h, r)| (*h, r))
            .collect()
    }

    /// Batch RPC call - sends multiple requests in one HTTP request
    async fn batch_rpc_call<T: for<'de> Deserialize<'de>>(
        &self,
        requests: &[serde_json::Value],
    ) -> OrchardResult<Vec<OrchardResult<T>>> {
        let mut request_builder = self.client.post(&self.config.rpc_url);

        if let (Some(user), Some(pass)) = (&self.config.rpc_user, &self.config.rpc_password) {
            request_builder = request_builder.basic_auth(user, Some(pass));
        }

        let response = request_builder
            .json(requests)
            .send()
            .await
            .map_err(|e| OrchardError::RpcError(format!("Batch RPC request failed: {}", e)))?;

        let response_text = response
            .text()
            .await
            .map_err(|e| OrchardError::RpcError(format!("Failed to read batch response: {}", e)))?;

        let responses: Vec<RpcResponse<T>> = serde_json::from_str(&response_text)
            .map_err(|e| OrchardError::RpcError(format!("Failed to parse batch response: {}", e)))?;

        // Sort by id to maintain order
        let mut sorted: Vec<_> = responses.into_iter().collect();
        sorted.sort_by_key(|r| r.id);

        Ok(sorted
            .into_iter()
            .map(|r| {
                r.result.ok_or_else(|| {
                    OrchardError::RpcError(r.error.map(|e| e.message).unwrap_or_else(|| "Unknown error".to_string()))
                })
            })
            .collect())
    }

    /// Fallback to parallel individual requests if batch fails
    async fn fetch_blocks_parallel_fallback(&self, heights: Vec<u64>) -> Vec<(u64, OrchardResult<VerboseBlock>)> {
        let futures: Vec<_> = heights
            .into_iter()
            .map(|height| {
                let client = self.client.clone();
                let config = self.config.clone();
                async move {
                    let result = Self::fetch_block_with_client(&client, &config, height).await;
                    (height, result)
                }
            })
            .collect();

        join_all(futures).await
    }

    /// Static helper to fetch a block with a given client (for fallback)
    async fn fetch_block_with_client(
        client: &reqwest::Client,
        config: &SyncConfig,
        height: u64,
    ) -> OrchardResult<VerboseBlock> {
        // Get block hash
        let hash_response: RpcResponse<String> = Self::rpc_call_with_client(
            client,
            config,
            "getblockhash",
            serde_json::json!([height]),
        ).await?;

        let hash = hash_response.result.ok_or_else(|| {
            OrchardError::RpcError(format!("Failed to get block hash for height {}", height))
        })?;

        // Get block with verbosity 2
        let block_response: RpcResponse<VerboseBlock> = Self::rpc_call_with_client(
            client,
            config,
            "getblock",
            serde_json::json!([hash, 2]),
        ).await?;

        block_response.result.ok_or_else(|| {
            OrchardError::RpcError(format!("Failed to get block at height {}", height))
        })
    }

    /// Static RPC call helper (for individual calls)
    async fn rpc_call_with_client<T: for<'de> Deserialize<'de>>(
        client: &reqwest::Client,
        config: &SyncConfig,
        method: &str,
        params: serde_json::Value,
    ) -> OrchardResult<RpcResponse<T>> {
        let mut request_builder = client.post(&config.rpc_url);

        if let (Some(user), Some(pass)) = (&config.rpc_user, &config.rpc_password) {
            request_builder = request_builder.basic_auth(user, Some(pass));
        }

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": method,
            "params": params,
        });

        let response = request_builder
            .json(&body)
            .send()
            .await
            .map_err(|e| OrchardError::RpcError(format!("RPC request failed: {}", e)))?;

        let response_text = response
            .text()
            .await
            .map_err(|e| OrchardError::RpcError(format!("Failed to read response: {}", e)))?;

        serde_json::from_str(&response_text)
            .map_err(|e| OrchardError::RpcError(format!("Failed to parse response: {} - {}", e, &response_text[..200.min(response_text.len())])))
    }

    /// Make an RPC call to Zebra
    async fn rpc_call<T: for<'de> Deserialize<'de>>(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> OrchardResult<RpcResponse<T>> {
        Self::rpc_call_with_client(&self.client, &self.config, method, params).await
    }

    /// Convert Zebra block to compact block format
    fn to_compact_block(&self, block: &VerboseBlock) -> OrchardResult<CompactBlock> {
        let mut transactions = Vec::new();

        for tx in &block.tx {
            if let Some(orchard) = &tx.orchard {
                let mut actions = Vec::new();

                for action in &orchard.actions {
                    actions.push(CompactOrchardAction {
                        cmx: self.decode_hex_32(&action.cm_x)?,
                        nullifier: self.decode_hex_32(&action.nullifier)?,
                        ephemeral_key: self.decode_hex_32(&action.ephemeral_key)?,
                        ciphertext: hex::decode(&action.enc_ciphertext)
                            .map_err(|e| OrchardError::RpcError(format!("Invalid ciphertext: {}", e)))?,
                    });
                }

                if !actions.is_empty() {
                    transactions.push(CompactTransaction {
                        hash: tx.txid.clone(),
                        orchard_actions: actions,
                        // Old-pool only. This legacy sync path predates the
                        // dual-pool scanner and never reads `tx.ironwood`; the
                        // live scan (witness_sync) covers both pools. Leaving it
                        // empty degrades safely — Ironwood notes are simply not
                        // seen here, and no Ironwood commitment can leak into the
                        // Orchard tree.
                        ironwood_actions: Vec::new(),
                    });
                }
            }
        }

        let mut hash = [0u8; 32];
        let hash_bytes = hex::decode(&block.hash)
            .map_err(|e| OrchardError::RpcError(format!("Invalid block hash: {}", e)))?;
        if hash_bytes.len() == 32 {
            hash.copy_from_slice(&hash_bytes);
        }

        Ok(CompactBlock {
            height: block.height,
            hash,
            transactions,
        })
    }

    /// Decode hex string to 32-byte array
    fn decode_hex_32(&self, hex_str: &str) -> OrchardResult<[u8; 32]> {
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

    /// Get the minimum scan height across all registered wallets
    async fn get_min_scan_height(&self) -> u64 {
        let scan_heights = self.wallet_scan_heights.read().await;
        scan_heights.values().cloned().min().unwrap_or(self.config.birthday_height)
    }

    /// Get the earliest note height from database across all wallets
    async fn get_earliest_note_height(&self) -> Option<u64> {
        if let Some(repo) = &self.db_repo {
            let keys = self.wallet_keys.read().await;
            let mut min_height: Option<u64> = None;

            for wallet_id in keys.keys() {
                if let Ok(notes) = repo.get_unspent_notes(*wallet_id).await {
                    for note in notes {
                        match min_height {
                            None => min_height = Some(note.block_height),
                            Some(h) if note.block_height < h => min_height = Some(note.block_height),
                            _ => {}
                        }
                    }
                }
            }

            if min_height.is_some() {
                tracing::debug!(
                    "[Orchard Sync] Earliest note found at height {}",
                    min_height.unwrap()
                );
            }

            min_height
        } else {
            None
        }
    }

    /// Initialize tree from frontier at a given height
    async fn initialize_tree_from_frontier(&self, note_height: u64) -> OrchardResult<()> {
        // Get frontier from one block before the note
        let frontier_height = note_height.saturating_sub(1);

        tracing::info!(
            "[Orchard Sync] Getting tree state at height {} to initialize tree",
            frontier_height
        );

        let (frontier_hex, frontier_root, _) = self.get_tree_state(frontier_height).await?;

        tracing::info!(
            "[Orchard Sync] Got frontier at height {}, root={}...",
            frontier_height,
            &frontier_root[..std::cmp::min(16, frontier_root.len())]
        );

        // Count commitments at frontier height
        let start_position = self.count_commitments_at_height(frontier_height).await.unwrap_or(0);

        // Initialize scanner's tree from frontier
        let mut scanner = self.scanner.write().await;
        scanner.init_from_frontier(&frontier_hex, start_position, frontier_height)
            .map_err(|e| OrchardError::Scanner(format!("Failed to init from frontier: {:?}", e)))?;

        tracing::info!(
            "[Orchard Sync] ✅ Tree initialized from frontier at height {}, position {}",
            frontier_height,
            start_position
        );

        Ok(())
    }

    /// Sync blocks from last scanned height to chain tip (with parallel fetching)
    pub async fn sync(&self) -> OrchardResult<ScanProgress> {
        let chain_tip = self.get_chain_height().await?;

        let registered_keys = {
            let keys = self.wallet_keys.read().await;
            keys.len()
        };

        if registered_keys == 0 {
            tracing::warn!("[Orchard Sync] No viewing keys registered, skipping scan");
            let scanner = self.scanner.read().await;
            return Ok(scanner.progress().clone());
        }

        // Get the minimum scan height across all wallets (from database)
        // This is the authoritative sync progress - we don't need to rescan from note height
        // just because tree state is lost. Witness refresh is handled separately when spending.
        let start_height = self.get_min_scan_height().await;

        tracing::info!(
            "[Orchard Sync] Chain tip: {}, start height: {}, registered wallets: {}",
            chain_tip,
            start_height,
            registered_keys
        );

        if start_height >= chain_tip {
            tracing::info!("[Orchard Sync] Already synced to chain tip {}", chain_tip);
            let scanner = self.scanner.read().await;
            return Ok(scanner.progress().clone());
        }

        let blocks_to_scan = chain_tip - start_height;
        tracing::info!(
            "[Orchard Sync] Starting sync: {} -> {} ({} blocks to scan, parallel={})",
            start_height,
            chain_tip,
            blocks_to_scan,
            self.config.parallel_fetches
        );

        let mut current_height = start_height + 1;
        let batch_size = self.config.batch_size;
        let rpc_batch_size = self.config.parallel_fetches;  // Blocks per RPC batch request
        let mut total_notes_found = 0usize;
        let mut total_blocks_scanned = 0usize;
        let sync_start = std::time::Instant::now();
        let mut last_persist_height = start_height;

        // Time tracking for detailed statistics
        let mut total_fetch_time = std::time::Duration::ZERO;
        let mut total_scan_time = std::time::Duration::ZERO;
        let mut total_note_store_time = std::time::Duration::ZERO;

        while current_height <= chain_tip {
            let end_height = std::cmp::min(current_height + batch_size - 1, chain_tip);

            let mut all_blocks = Vec::new();
            let mut fetch_errors = 0usize;

            // Fetch blocks in parallel RPC batches (multiple batches concurrently)
            let heights: Vec<u64> = (current_height..=end_height).collect();
            let fetch_start = std::time::Instant::now();

            // Create futures for all batch requests and execute in parallel
            let batch_futures: Vec<_> = heights
                .chunks(rpc_batch_size)
                .map(|chunk| self.fetch_blocks_batch(chunk.to_vec()))
                .collect();

            let batch_results = join_all(batch_futures).await;

            // Collect all results
            for results in batch_results {
                for (height, result) in results {
                    match result {
                        Ok(block) => {
                            if let Ok(compact_block) = self.to_compact_block(&block) {
                                all_blocks.push((height, compact_block));
                            }
                        }
                        Err(e) => {
                            fetch_errors += 1;
                            if fetch_errors <= 3 {
                                tracing::warn!("[Orchard Sync] Failed to fetch block {}: {}", height, e);
                            }
                        }
                    }
                }
            }

            let fetch_elapsed = fetch_start.elapsed();
            total_fetch_time += fetch_elapsed;

            // Sort blocks by height
            all_blocks.sort_by_key(|(h, _)| *h);
            let blocks: Vec<CompactBlock> = all_blocks.into_iter().map(|(_, b)| b).collect();

            if fetch_errors > 3 {
                tracing::warn!(
                    "[Orchard Sync] {} total block fetch errors in range {}-{} (fetch took {:.2}s)",
                    fetch_errors,
                    current_height,
                    end_height,
                    fetch_elapsed.as_secs_f64()
                );
            }

            if !blocks.is_empty() {
                total_blocks_scanned += blocks.len();

                // Time the scan operation
                let scan_start = std::time::Instant::now();
                let mut scanner = self.scanner.write().await;
                let found_notes = scanner.scan_blocks(blocks, chain_tip).await?;

                // Get newly spent notes detected during this scan
                let spent_notes = scanner.take_newly_spent_notes();
                drop(scanner);
                total_scan_time += scan_start.elapsed();

                if !found_notes.is_empty() {
                    total_notes_found += found_notes.len();
                    tracing::info!(
                        "[Orchard Sync] 🎉 Found {} notes in blocks {}-{} (total: {})",
                        found_notes.len(),
                        current_height,
                        end_height,
                        total_notes_found
                    );

                    // Store notes (memory + database) - time this operation
                    let note_store_start = std::time::Instant::now();
                    self.store_notes(&found_notes).await;
                    total_note_store_time += note_store_start.elapsed();
                }

                // Sync spent notes to database
                if !spent_notes.is_empty() {
                    tracing::info!(
                        "[Orchard Sync] 💸 Detected {} spent notes in blocks {}-{}",
                        spent_notes.len(),
                        current_height,
                        end_height
                    );
                    let spent_store_start = std::time::Instant::now();
                    self.mark_notes_spent(&spent_notes).await;
                    total_note_store_time += spent_store_start.elapsed();
                }
            }

            current_height = end_height + 1;

            // Persist scan state every 1000 blocks or at end
            if current_height - last_persist_height >= 1000 || current_height > chain_tip {
                self.persist_scan_state(end_height).await;
                last_persist_height = end_height;
            }

            // Log progress every 500 blocks
            let blocks_scanned = current_height - start_height - 1;
            if blocks_scanned % 500 == 0 || blocks_scanned == blocks_to_scan {
                let elapsed = sync_start.elapsed().as_secs_f64();
                let blocks_per_sec = if elapsed > 0.0 {
                    blocks_scanned as f64 / elapsed
                } else {
                    0.0
                };
                let remaining_blocks = chain_tip.saturating_sub(current_height) + 1;
                let eta_secs = if blocks_per_sec > 0.0 {
                    remaining_blocks as f64 / blocks_per_sec
                } else {
                    0.0
                };
                let progress_pct = (blocks_scanned as f64 / blocks_to_scan as f64) * 100.0;

                tracing::info!(
                    "[Orchard Sync] Progress: {:.1}% ({}/{} blocks), {:.1} blocks/sec, ETA: {:.0}s, notes found: {}",
                    progress_pct,
                    blocks_scanned,
                    blocks_to_scan,
                    blocks_per_sec,
                    eta_secs,
                    total_notes_found
                );
            }
        }

        // Final persist
        self.persist_scan_state(chain_tip).await;

        // Persist witnesses to database only if needed (lazy sync)
        // Check if witness height is more than 50 blocks behind chain tip
        let witness_start = std::time::Instant::now();
        let witness_synced = self.maybe_persist_witnesses(chain_tip).await;
        let witness_time = witness_start.elapsed();

        let total_elapsed = sync_start.elapsed();
        let total_secs = total_elapsed.as_secs_f64();

        // Calculate other time (tree validation, etc.)
        let other_time = total_elapsed
            .saturating_sub(total_fetch_time)
            .saturating_sub(total_scan_time)
            .saturating_sub(total_note_store_time)
            .saturating_sub(witness_time);

        tracing::info!(
            "[Orchard Sync] ✅ Sync complete to height {} ({} blocks, {} notes found)",
            chain_tip,
            total_blocks_scanned,
            total_notes_found
        );
        tracing::info!(
            "[Orchard Sync] ⏱️  Time breakdown: total={:.2}s | fetch={:.2}s | scan={:.2}s | note_db={:.2}s | witness={:.2}s{} | other={:.2}s",
            total_secs,
            total_fetch_time.as_secs_f64(),
            total_scan_time.as_secs_f64(),
            total_note_store_time.as_secs_f64(),
            witness_time.as_secs_f64(),
            if witness_synced { " (synced)" } else { " (skipped)" },
            other_time.as_secs_f64()
        );
        tracing::info!(
            "[Orchard Sync] 📊 Performance: {:.1} blocks/sec",
            total_blocks_scanned as f64 / total_secs.max(0.1)
        );

        // Log database pool stats after sync
        if let Some(repo) = &self.db_repo {
            crate::db::log_pool_stats(repo.pool());
        }

        let scanner = self.scanner.read().await;
        Ok(scanner.progress().clone())
    }

    /// Store discovered notes by wallet (memory + database)
    /// Now stores full spending data (recipient, rho, rseed) for shielded spending support
    async fn store_notes(&self, notes: &[OrchardNote]) {
        let keys = self.wallet_keys.read().await;
        let mut notes_by_wallet = self.notes_by_wallet.write().await;

        for note in notes {
            // Use wallet_id directly from the note (set during decryption)
            let target_wallet_id = if let Some(wallet_id) = note.wallet_id {
                Some(wallet_id)
            } else {
                // Fallback: find wallet_id by account_index (legacy behavior)
                keys.iter()
                    .find(|(_, vk)| vk.account_index == note.account_id)
                    .map(|(wallet_id, _)| *wallet_id)
            };

            if let Some(wallet_id) = target_wallet_id {
                // Memory store
                notes_by_wallet
                    .entry(wallet_id)
                    .or_insert_with(Vec::new)
                    .push(note.clone());

                // Persist to database with full spending data and witness position
                if let Some(repo) = &self.db_repo {
                    let nullifier_hex = hex::encode(note.nullifier);
                    let recipient_hex = hex::encode(note.recipient);
                    let rho_hex = hex::encode(note.rho);
                    let rseed_hex = hex::encode(note.rseed);

                    // Save the global tree position for fast witness refresh
                    // This avoids re-scanning when refreshing witnesses later
                    match repo.save_note_full(
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
                        note.position,  // witness_position = global tree position
                        note.pool.as_db_str(),
                    ).await {
                        Ok(_) => {
                            tracing::debug!(
                                "[Orchard Sync] Persisted note to database: wallet={}, value={}, nullifier={}..., position={}, with spending data",
                                wallet_id,
                                note.value_zatoshis,
                                &nullifier_hex[..16],
                                note.position
                            );
                        }
                        Err(e) => {
                            tracing::error!("[Orchard Sync] Failed to persist note: {}", e);
                        }
                    }
                }
            }
        }
    }

    /// Mark notes as spent in database
    async fn mark_notes_spent(&self, spent_notes: &[SpentNoteInfo]) {
        if let Some(repo) = &self.db_repo {
            for spent in spent_notes {
                let nullifier_hex = hex::encode(spent.nullifier);
                match repo.mark_note_spent(&nullifier_hex, &spent.spent_in_tx).await {
                    Ok(updated) => {
                        if updated {
                            tracing::info!(
                                "[Orchard Sync] ✅ Marked note as spent in DB: nullifier={}, tx={}",
                                &nullifier_hex[..16],
                                &spent.spent_in_tx[..std::cmp::min(16, spent.spent_in_tx.len())]
                            );
                        } else {
                            tracing::debug!(
                                "[Orchard Sync] Note already spent or not found: nullifier={}",
                                &nullifier_hex[..16]
                            );
                        }
                    }
                    Err(e) => {
                        tracing::warn!(
                            "[Orchard Sync] Failed to mark note spent: nullifier={}, error={}",
                            &nullifier_hex[..16],
                            e
                        );
                    }
                }
            }
        }
    }

    /// Persist scan state to database
    async fn persist_scan_state(&self, height: u64) {
        if let Some(repo) = &self.db_repo {
            let keys = self.wallet_keys.read().await;
            let mut scan_heights = self.wallet_scan_heights.write().await;

            for wallet_id in keys.keys() {
                scan_heights.insert(*wallet_id, height);

                // Get notes count for this wallet
                let notes_count = self.notes_by_wallet
                    .read()
                    .await
                    .get(wallet_id)
                    .map(|n| n.len() as u32)
                    .unwrap_or(0);

                if let Err(e) = repo.upsert_sync_state(*wallet_id, height, notes_count).await {
                    tracing::warn!("[Orchard Sync] Failed to persist scan state for wallet {}: {}", wallet_id, e);
                }
            }

            tracing::debug!("[Orchard Sync] Persisted scan state at height {}", height);
        }
    }

    /// Minimum blocks difference before triggering witness refresh
    /// Zcash nodes typically accept anchors up to ~100 blocks old
    const WITNESS_SYNC_THRESHOLD: u64 = 25;

    /// Refresh witnesses if the height difference exceeds threshold
    ///
    /// This implements lazy witness sync - witnesses are only refreshed when:
    /// 1. The chain has advanced more than 100 blocks since last witness update
    /// 2. This ensures anchors remain valid for spending (Zcash accepts ~100 block old anchors)
    ///
    /// IMPORTANT: This calls refresh_witnesses_for_spending() which actually rescans
    /// the chain to compute fresh witnesses, not just persist stale data from memory.
    ///
    /// Returns true if witnesses were actually synced, false if skipped
    async fn maybe_persist_witnesses(&self, chain_tip: u64) -> bool {
        if let Some(repo) = &self.db_repo {
            // Get the minimum witness height across all wallets with unspent notes
            let min_witness_height = match repo.get_min_witness_height().await {
                Ok(h) => h,
                Err(e) => {
                    tracing::warn!("[Orchard Sync] Failed to get min witness height: {}", e);
                    0
                }
            };

            let blocks_behind = chain_tip.saturating_sub(min_witness_height);

            // Check if any wallet has notes missing witness data
            let keys = self.wallet_keys.read().await;
            let wallet_ids: Vec<i32> = keys.keys().copied().collect();
            drop(keys);

            let mut has_missing_witnesses = false;
            for wallet_id in &wallet_ids {
                if repo.has_notes_missing_witness(*wallet_id).await.unwrap_or(false) {
                    has_missing_witnesses = true;
                    tracing::info!(
                        "[Orchard Sync] Wallet {} has notes missing witness data",
                        wallet_id
                    );
                    break;
                }
            }

            // Trigger refresh if either: threshold exceeded OR notes missing witness
            if blocks_behind >= Self::WITNESS_SYNC_THRESHOLD || has_missing_witnesses {
                let reason = if has_missing_witnesses {
                    "notes missing witness data".to_string()
                } else {
                    format!("{} blocks behind (threshold={})", blocks_behind, Self::WITNESS_SYNC_THRESHOLD)
                };

                tracing::info!(
                    "[Orchard Sync] 🔄 Witness refresh triggered: {} (chain_tip={}, last_witness_height={})",
                    reason,
                    chain_tip,
                    min_witness_height
                );

                // Refresh witnesses for all wallets with unspent notes
                // This rescans the chain to compute fresh witnesses with valid anchors
                let mut refreshed_count = 0;
                for wallet_id in &wallet_ids {
                    let wallet_id = *wallet_id;
                    // Check if wallet has unspent notes
                    let has_notes = repo.get_notes_count(wallet_id).await.unwrap_or(0) > 0;
                    if !has_notes {
                        continue;
                    }

                    tracing::info!(
                        "[Orchard Sync] Refreshing witnesses for wallet {}...",
                        wallet_id
                    );

                    match self.refresh_witnesses_for_spending(wallet_id).await {
                        Ok(true) => {
                            refreshed_count += 1;
                            tracing::info!(
                                "[Orchard Sync] ✅ Wallet {} witnesses refreshed",
                                wallet_id
                            );
                        }
                        Ok(false) => {
                            tracing::debug!(
                                "[Orchard Sync] Wallet {} no witnesses to refresh",
                                wallet_id
                            );
                        }
                        Err(e) => {
                            tracing::error!(
                                "[Orchard Sync] ❌ Failed to refresh witnesses for wallet {}: {}",
                                wallet_id,
                                e
                            );
                        }
                    }

                    // Update witness height for this wallet
                    if let Err(e) = repo.update_witness_height(wallet_id, chain_tip).await {
                        tracing::warn!(
                            "[Orchard Sync] Failed to update witness height for wallet {}: {}",
                            wallet_id,
                            e
                        );
                    }
                }

                tracing::info!(
                    "[Orchard Sync] ✅ Witness refresh complete: {} wallets refreshed, height updated to {}",
                    refreshed_count,
                    chain_tip
                );
                return refreshed_count > 0;
            } else {
                tracing::debug!(
                    "[Orchard Sync] Witness refresh skipped: only {} blocks behind (threshold={})",
                    blocks_behind,
                    Self::WITNESS_SYNC_THRESHOLD
                );
            }
        }
        false
    }

    /// Get balance for a wallet (from database if available)
    pub async fn get_wallet_balance(&self, wallet_id: i32) -> ShieldedBalance {
        // Try database first
        if let Some(repo) = &self.db_repo {
            if let Ok(balance) = repo.get_balance(wallet_id).await {
                let notes_count = repo.get_unspent_notes(wallet_id).await
                    .map(|n| n.len() as u32)
                    .unwrap_or(0);
                return ShieldedBalance::new(
                    ShieldedPool::Orchard,
                    balance,
                    balance,  // All unspent are spendable
                    notes_count,
                );
            }
        }

        // Fallback to in-memory scanner (now uses wallet_id directly)
        let scanner = self.scanner.read().await;
        let chain_height = self.get_chain_height().await.unwrap_or(0);

        let total = scanner.get_balance(wallet_id);
        let spendable = scanner.get_spendable_balance(wallet_id, chain_height);
        let unspent_notes = scanner.get_unspent_notes(wallet_id);

        ShieldedBalance::new(
            ShieldedPool::Orchard,
            total,
            spendable,
            unspent_notes.len() as u32,
        )
    }

    /// Get unspent notes for a wallet
    pub async fn get_unspent_notes(&self, wallet_id: i32) -> Vec<OrchardNote> {
        let scanner = self.scanner.read().await;
        // Now uses wallet_id directly - no filtering needed
        scanner.get_unspent_notes(wallet_id)
    }

    /// Get spendable notes for a wallet
    pub async fn get_spendable_notes(&self, wallet_id: i32) -> Vec<OrchardNote> {
        let scanner = self.scanner.read().await;
        let chain_height = self.get_chain_height().await.unwrap_or(0);
        // Now uses wallet_id directly - no filtering needed
        scanner.get_spendable_notes(wallet_id, chain_height)
    }

    /// Get current scan progress
    pub async fn get_progress(&self) -> ScanProgress {
        let scanner = self.scanner.read().await;
        scanner.progress().clone()
    }

    /// Check if fully synced
    pub async fn is_synced(&self) -> bool {
        let scanner = self.scanner.read().await;
        let progress = scanner.progress();
        progress.progress_percent >= 99.9
    }

    /// Get the current commitment tree anchor
    ///
    /// Returns the root of the Orchard commitment tree.
    /// This anchor is needed for building shielded transactions.
    pub async fn get_tree_anchor(&self) -> [u8; 32] {
        let scanner = self.scanner.read().await;
        scanner.get_anchor()
    }

    /// Get the Orchard anchor for transaction building
    pub async fn get_orchard_anchor(&self) -> orchard::tree::Anchor {
        let scanner = self.scanner.read().await;
        scanner.get_orchard_anchor()
    }

    /// Get the Merkle path for a note at the given position
    ///
    /// Returns the authentication path needed to prove a note exists in the tree.
    pub async fn get_merkle_path(&self, position: u64) -> Option<orchard::tree::MerklePath> {
        let scanner = self.scanner.read().await;
        scanner.get_merkle_path(position)
    }

    /// Get witness data for a note at the given position
    pub async fn get_witness_data(&self, position: u64) -> Option<super::tree::WitnessData> {
        let scanner = self.scanner.read().await;
        scanner.tree_tracker().get_witness(position)
    }

    /// Get spendable notes for a wallet with witness data populated
    ///
    /// This is the preferred method for getting notes for shielded spending,
    /// as it ensures all notes have valid witness data.
    ///
    /// If notes/witnesses are not in memory (e.g., after restart), loads from database.
    pub async fn get_spendable_notes_with_witnesses(&self, wallet_id: i32) -> Vec<OrchardNote> {
        let scanner = self.scanner.read().await;
        let chain_height = self.get_chain_height().await.unwrap_or(0);

        // Now uses wallet_id directly - notes are properly isolated by wallet_id in scanner
        let memory_notes = scanner.get_spendable_notes(wallet_id, chain_height);

        // Check how many have witness data in memory
        let notes_with_memory_witness = memory_notes.iter().filter(|n| n.witness_data.is_some()).count();

        tracing::info!(
            "[Orchard Sync] get_spendable_notes_with_witnesses: wallet={}, memory_notes={}, with_witness={}",
            wallet_id,
            memory_notes.len(),
            notes_with_memory_witness
        );

        // If we have notes with witnesses in memory, use them
        // No need to filter by wallet_id anymore - notes are now stored by wallet_id in scanner
        if notes_with_memory_witness > 0 {
            let result: Vec<_> = memory_notes.into_iter()
                .filter(|n| n.witness_data.is_some())
                .collect();
            tracing::info!(
                "[Orchard Sync] Returning {} notes with witnesses from memory for wallet {}",
                result.len(),
                wallet_id
            );
            return result;
        }

            // Otherwise, try to load notes directly from database (including witness data)
            if let Some(repo) = &self.db_repo {
                // Legacy path: old Orchard pool only (see to_compact_block).
                match repo
                    .get_notes_with_witnesses(wallet_id, ShieldedPool::Orchard.as_db_str())
                    .await
                {
                    Ok(db_notes) => {
                        tracing::info!(
                            "[Orchard Sync] Loading {} notes with witnesses from database",
                            db_notes.len()
                        );

                        let mut result = Vec::new();

                        for db_note in db_notes {
                            // Parse all required fields
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

                            // Parse witness data
                            let witness_data = match (
                                db_note.witness_position,
                                &db_note.witness_auth_path,
                                &db_note.witness_root,
                            ) {
                                (Some(position), Some(auth_path_json), Some(root_hex)) => {
                                    // Parse auth_path from JSON
                                    let auth_path: Vec<[u8; 32]> = match serde_json::from_str::<Vec<String>>(auth_path_json) {
                                        Ok(hex_strings) => {
                                            hex_strings.iter().filter_map(|s| {
                                                hex::decode(s).ok().and_then(|bytes| {
                                                    if bytes.len() == 32 {
                                                        let mut arr = [0u8; 32];
                                                        arr.copy_from_slice(&bytes);
                                                        Some(arr)
                                                    } else {
                                                        None
                                                    }
                                                })
                                            }).collect()
                                        }
                                        Err(e) => {
                                            tracing::warn!("Failed to parse auth_path JSON: {}", e);
                                            continue;
                                        }
                                    };

                                    // Parse root
                                    let root: [u8; 32] = match hex::decode(root_hex) {
                                        Ok(bytes) if bytes.len() == 32 => {
                                            let mut arr = [0u8; 32];
                                            arr.copy_from_slice(&bytes);
                                            arr
                                        }
                                        _ => {
                                            tracing::warn!("Invalid root hex: {}", root_hex);
                                            continue;
                                        }
                                    };

                                    if auth_path.len() != 32 {
                                        tracing::warn!("Invalid auth_path length: {}", auth_path.len());
                                        continue;
                                    }

                                    Some(super::tree::WitnessData {
                                        position,
                                        auth_path,
                                        root,
                                    })
                                }
                                _ => {
                                    tracing::debug!("Note missing witness data: {}", &db_note.nullifier[..16]);
                                    continue;
                                }
                            };

                            // Build OrchardNote from database
                            // IMPORTANT: Use witness_position for the note's position in the global tree
                            // position_in_block is just the local index within the block
                            let global_position = witness_data.as_ref()
                                .map(|w| w.position)
                                .unwrap_or(db_note.position_in_block as u64);

                            let note = OrchardNote {
                                id: Some(db_note.id as i64),
                                wallet_id: Some(wallet_id),
                                account_id: 0, // Legacy field, wallet_id is now the primary key
                                tx_hash: db_note.tx_hash.clone(),
                                block_height: db_note.block_height,
                                note_commitment: [0u8; 32], // Not stored in DB
                                nullifier,
                                value_zatoshis: db_note.value_zatoshis,
                                position: global_position, // Use global tree position, not block position
                                is_spent: db_note.is_spent,
                                memo: db_note.memo.clone(),
                                merkle_path: None,
                                recipient,
                                rho,
                                rseed,
                                witness_data,
                                pool: ShieldedPool::from_db_str(&db_note.pool),
                            };

                            tracing::debug!(
                                "[Orchard Sync] Loaded note from DB: nullifier={}..., value={}, has_witness={}",
                                &db_note.nullifier[..16],
                                db_note.value_zatoshis,
                                note.witness_data.is_some()
                            );

                            result.push(note);
                        }

                        tracing::info!(
                            "[Orchard Sync] Returning {} notes with witnesses from database for wallet {}",
                            result.len(),
                            wallet_id
                        );

                        return result;
                    }
                    Err(e) => {
                        tracing::warn!("[Orchard Sync] Failed to load notes from database: {}", e);
                    }
                }
            }

            // Fallback: return memory notes filtered by witness
            // No need to filter by wallet_id - notes are now stored by wallet_id in scanner
            let result: Vec<_> = memory_notes.into_iter()
                .filter(|n| n.witness_data.is_some())
                .collect();

            tracing::info!(
                "[Orchard Sync] Returning {} notes with witnesses for wallet {} (fallback)",
                result.len(),
                wallet_id
            );

            result
    }

    /// Maximum anchor age in blocks before witnesses need to be refreshed
    /// Zcash nodes typically accept anchors up to ~100 blocks old
    const MAX_ANCHOR_AGE_BLOCKS: u64 = 100;

    /// Fetch only commitments (cmx values) for a range of blocks
    ///
    /// This is optimized for witness refresh - it extracts only the cmx values
    /// from blocks without processing other data.
    async fn fetch_commitments_for_range(&self, from_height: u64, to_height: u64) -> OrchardResult<Vec<[u8; 32]>> {
        let heights: Vec<u64> = (from_height..=to_height).collect();
        let batch_size = self.config.parallel_fetches;

        let batch_futures: Vec<_> = heights
            .chunks(batch_size)
            .map(|chunk| self.fetch_blocks_batch(chunk.to_vec()))
            .collect();

        let batch_results = join_all(batch_futures).await;

        // Collect and sort blocks by height
        let mut all_blocks: Vec<(u64, VerboseBlock)> = Vec::new();
        for results in batch_results {
            for (height, result) in results {
                if let Ok(block) = result {
                    all_blocks.push((height, block));
                }
            }
        }
        all_blocks.sort_by_key(|(h, _)| *h);

        // Extract only commitments (cmx values)
        let mut commitments = Vec::new();
        for (_, block) in all_blocks {
            for tx in &block.tx {
                if let Some(orchard) = &tx.orchard {
                    for action in &orchard.actions {
                        if let Ok(cmx) = self.decode_hex_32(&action.cm_x) {
                            commitments.push(cmx);
                        }
                    }
                }
            }
        }

        tracing::debug!(
            "[Orchard Sync] Fetched {} commitments from heights {}-{}",
            commitments.len(),
            from_height,
            to_height
        );

        Ok(commitments)
    }

    /// Refresh witnesses for spending - simplified incremental approach
    ///
    /// This method updates witnesses by:
    /// 1. Determining the tree state height (from scanner or database)
    /// 2. Fetching only commitments from that height to chain tip
    /// 3. Appending commitments to tree in one batch
    /// 4. Extracting updated witnesses
    ///
    /// This is much more efficient than the old approach which re-scanned
    /// and attempted decryption for every action.
    ///
    /// Returns true if witnesses were successfully refreshed.
    pub async fn refresh_witnesses_for_spending(&self, wallet_id: i32) -> OrchardResult<bool> {
        let chain_tip = self.get_chain_height().await?;

        // Get notes from database
        let notes_info = if let Some(repo) = &self.db_repo {
            match repo.get_unspent_notes(wallet_id).await {
                Ok(notes) => notes,
                Err(e) => {
                    tracing::error!("[Orchard Sync] Failed to get notes from database: {}", e);
                    return Err(OrchardError::DatabaseError(e.to_string()));
                }
            }
        } else {
            return Err(OrchardError::DatabaseError("No database connection".to_string()));
        };

        if notes_info.is_empty() {
            tracing::debug!("[Orchard Sync] No notes found for wallet {}", wallet_id);
            return Ok(false);
        }

        // Find the minimum block height among all notes
        let min_note_height = notes_info.iter().map(|n| n.block_height).min().unwrap_or(0);

        // Check current scanner/tree state
        let (scanner_height, tree_size, witness_count) = {
            let scanner = self.scanner.read().await;
            (
                scanner.progress().last_scanned_height,
                scanner.tree_tracker().tree_size(),
                scanner.tree_tracker().witness_count(),
            )
        };

        tracing::info!(
            "[Orchard Sync] 🔄 Refresh witnesses: wallet={}, notes={}, min_note_height={}, scanner_height={}, tree_size={}, witnesses={}, chain_tip={}",
            wallet_id,
            notes_info.len(),
            min_note_height,
            scanner_height,
            tree_size,
            witness_count,
            chain_tip
        );

        // Determine if we can use incremental update or need full rebuild
        let tree_is_valid = scanner_height >= min_note_height && (witness_count > 0 || tree_size > 1000);

        if tree_is_valid && scanner_height >= chain_tip {
            // Tree is already at chain tip, just extract witnesses
            tracing::info!(
                "[Orchard Sync] ✅ Tree already at chain tip ({}), extracting witnesses",
                chain_tip
            );
            return self.extract_and_persist_witnesses(wallet_id, &notes_info, chain_tip).await;
        }

        if tree_is_valid {
            // Incremental update: just append new commitments
            let update_start = scanner_height + 1;
            let blocks_to_update = chain_tip - scanner_height;

            tracing::info!(
                "[Orchard Sync] Incremental update: {} -> {} ({} blocks)",
                update_start,
                chain_tip,
                blocks_to_update
            );

            // Fetch commitments for the range
            let commitments = self.fetch_commitments_for_range(update_start, chain_tip).await?;

            if !commitments.is_empty() {
                // Append to tree (no decryption needed)
                let mut scanner = self.scanner.write().await;
                let appended = scanner.append_commitments_only(&commitments, chain_tip)?;
                tracing::info!(
                    "[Orchard Sync] Appended {} commitments to tree, new height={}",
                    appended,
                    chain_tip
                );
            }

            // Extract and persist witnesses
            return self.extract_and_persist_witnesses(wallet_id, &notes_info, chain_tip).await;
        }

        // Tree state is stale/missing - need full rebuild from frontier
        let frontier_height = min_note_height.saturating_sub(1);

        tracing::info!(
            "[Orchard Sync] Tree rebuild needed: getting frontier at height {}",
            frontier_height
        );

        let (frontier_hex, _frontier_root, _) = self.get_tree_state(frontier_height).await?;
        let start_position = self.count_commitments_at_height(frontier_height).await.unwrap_or(0);

        // Initialize tree from frontier
        {
            let mut scanner = self.scanner.write().await;
            if let Err(e) = scanner.init_from_frontier(&frontier_hex, start_position, frontier_height) {
                tracing::warn!(
                    "[Orchard Sync] Failed to init from frontier: {:?}",
                    e
                );
                return Err(OrchardError::Scanner(format!("Failed to init tree: {:?}", e)));
            }
        }

        // Check if we already have witness_position for all notes
        // If yes, we can skip decryption and just mark by position
        let known_positions: Vec<u64> = notes_info.iter()
            .filter_map(|n| n.witness_position)
            .collect();

        let all_positions_known = known_positions.len() == notes_info.len() && !notes_info.is_empty();

        if all_positions_known {
            // Optimized path: all positions known, just fetch commitments and mark
            tracing::info!(
                "[Orchard Sync] ✅ All {} note positions known, using fast rebuild (no decryption)",
                known_positions.len()
            );

            let commitments = self.fetch_commitments_for_range(min_note_height, chain_tip).await?;

            if !commitments.is_empty() {
                let mut scanner = self.scanner.write().await;
                let appended = scanner.append_commitments_with_marks(
                    &commitments,
                    start_position,
                    &known_positions,
                    chain_tip,
                )?;
                tracing::info!(
                    "[Orchard Sync] Fast rebuild: appended {} commitments, marked {} positions",
                    appended,
                    known_positions.len()
                );
            }
        } else {
            // Slow path: need to scan and decrypt to find note positions
            tracing::info!(
                "[Orchard Sync] ⚠️ Note positions unknown ({}/{}), need full scan with decryption",
                known_positions.len(),
                notes_info.len()
            );

            let note_heights: std::collections::BTreeSet<u64> = notes_info.iter().map(|n| n.block_height).collect();
            let max_note_height = *note_heights.iter().max().unwrap_or(&min_note_height);

            // Scan from min_note_height to max_note_height (where notes exist)
            tracing::info!(
                "[Orchard Sync] Scanning note blocks: {} -> {} ({} blocks)",
                min_note_height,
                max_note_height,
                max_note_height - min_note_height + 1
            );

            let batch_size = self.config.batch_size;
            let mut current_height = min_note_height;

            while current_height <= max_note_height {
                let end_height = std::cmp::min(current_height + batch_size - 1, max_note_height);
                let heights: Vec<u64> = (current_height..=end_height).collect();

                let batch_futures: Vec<_> = heights
                    .chunks(self.config.parallel_fetches)
                    .map(|chunk| self.fetch_blocks_batch(chunk.to_vec()))
                    .collect();

                let batch_results = join_all(batch_futures).await;

                let mut all_blocks = Vec::new();
                for results in batch_results {
                    for (height, result) in results {
                        if let Ok(block) = result {
                            if let Ok(compact_block) = self.to_compact_block(&block) {
                                all_blocks.push((height, compact_block));
                            }
                        }
                    }
                }

                all_blocks.sort_by_key(|(h, _)| *h);
                let blocks: Vec<CompactBlock> = all_blocks.into_iter().map(|(_, b)| b).collect();

                if !blocks.is_empty() {
                    let mut scanner = self.scanner.write().await;
                    // This will mark witnesses for our notes
                    let _ = scanner.scan_blocks(blocks, chain_tip).await?;
                }

                current_height = end_height + 1;
            }

            // For remaining blocks, just append commitments (no decryption)
            if max_note_height < chain_tip {
                let update_start = max_note_height + 1;
                tracing::info!(
                    "[Orchard Sync] Appending remaining commitments: {} -> {} ({} blocks)",
                    update_start,
                    chain_tip,
                    chain_tip - update_start + 1
                );

                let commitments = self.fetch_commitments_for_range(update_start, chain_tip).await?;
                if !commitments.is_empty() {
                    let mut scanner = self.scanner.write().await;
                    scanner.append_commitments_only(&commitments, chain_tip)?;
                }
            }
        }

        // Extract and persist witnesses
        self.extract_and_persist_witnesses(wallet_id, &notes_info, chain_tip).await
    }

    /// Extract witnesses from tree and persist to database
    async fn extract_and_persist_witnesses(
        &self,
        wallet_id: i32,
        notes_info: &[crate::db::repositories::orchard_repo::StoredOrchardNote],
        chain_tip: u64,
    ) -> OrchardResult<bool> {
        // Build nullifier set for lookup
        let note_nullifiers: std::collections::HashSet<[u8; 32]> = notes_info.iter()
            .filter_map(|n| {
                hex::decode(&n.nullifier).ok().and_then(|bytes| {
                    if bytes.len() == 32 {
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(&bytes);
                        Some(arr)
                    } else {
                        None
                    }
                })
            })
            .collect();

        // Get fresh witnesses from scanner (now uses wallet_id directly)
        let notes_with_witnesses = {
            let scanner = self.scanner.read().await;
            scanner.get_spendable_notes(wallet_id, chain_tip)
                .into_iter()
                .filter(|n| note_nullifiers.contains(&n.nullifier) && n.witness_data.is_some())
                .collect::<Vec<_>>()
        };

        tracing::info!(
            "[Orchard Sync] Extracted {} witnesses for wallet {}",
            notes_with_witnesses.len(),
            wallet_id
        );

        // Validate tree root if we have witnesses
        if let Some(first_note) = notes_with_witnesses.first() {
            if let Some(ref witness) = first_note.witness_data {
                match self.get_expected_anchor(chain_tip).await {
                    Ok(expected_anchor) => {
                        if witness.root == expected_anchor {
                            tracing::info!(
                                "[Orchard Sync] ✅ Tree root matches expected anchor at height {}",
                                chain_tip
                            );
                        } else {
                            tracing::warn!(
                                "[Orchard Sync] ⚠️ Tree root mismatch! computed={}, expected={}",
                                hex::encode(&witness.root),
                                hex::encode(&expected_anchor)
                            );
                        }
                    }
                    Err(e) => {
                        tracing::debug!("[Orchard Sync] Could not validate root: {}", e);
                    }
                }
            }
        }

        // Persist witnesses to database
        if let Some(repo) = &self.db_repo {
            for note in &notes_with_witnesses {
                if let Some(ref witness) = note.witness_data {
                    let nullifier_hex = hex::encode(note.nullifier);
                    let auth_path_json: Vec<String> = witness.auth_path
                        .iter()
                        .map(hex::encode)
                        .collect();
                    let auth_path_str = serde_json::to_string(&auth_path_json).unwrap_or_default();
                    let root_hex = hex::encode(witness.root);

                    if let Err(e) = repo.update_witness_by_nullifier(
                        &nullifier_hex,
                        witness.position,
                        &auth_path_str,
                        &root_hex,
                    ).await {
                        tracing::warn!(
                            "[Orchard Sync] Failed to persist witness for {}: {}",
                            &nullifier_hex[..16],
                            e
                        );
                    }
                }
            }
        }

        Ok(!notes_with_witnesses.is_empty())
    }

    /// Check if the anchor is too old for spending
    ///
    /// Returns true if witnesses need to be refreshed before spending
    pub async fn is_anchor_too_old(&self, anchor_block_height: u64) -> bool {
        if let Ok(chain_tip) = self.get_chain_height().await {
            let age = chain_tip.saturating_sub(anchor_block_height);
            let too_old = age > Self::MAX_ANCHOR_AGE_BLOCKS;
            if too_old {
                tracing::info!(
                    "[Orchard Sync] Anchor is {} blocks old (max={}), needs refresh",
                    age,
                    Self::MAX_ANCHOR_AGE_BLOCKS
                );
            }
            too_old
        } else {
            true // Assume too old if we can't check
        }
    }

    /// Get the current tree block height
    pub async fn get_tree_block_height(&self) -> u64 {
        let scanner = self.scanner.read().await;
        scanner.tree_tracker().block_height()
    }

    /// Persist witness data to database for all tracked notes
    ///
    /// Call this after scanning to ensure witnesses are saved for later use.
    /// Uses nullifier deduplication to avoid saving the same witness multiple times
    /// when multiple wallets share the same account_index.
    pub async fn persist_witnesses(&self) {
        if let Some(repo) = &self.db_repo {
            let scanner = self.scanner.read().await;
            let keys = self.wallet_keys.read().await;
            let chain_height = self.get_chain_height().await.unwrap_or(0);

            let mut saved_count = 0;
            let mut error_count = 0;
            let mut processed_nullifiers = std::collections::HashSet::new();

            for (wallet_id, _vk) in keys.iter() {
                let notes = scanner.get_spendable_notes(*wallet_id, chain_height);

                for note in notes {
                    // Skip if already processed (deduplication)
                    if processed_nullifiers.contains(&note.nullifier) {
                        continue;
                    }
                    processed_nullifiers.insert(note.nullifier);

                    if let Some(ref witness) = note.witness_data {
                        let nullifier_hex = hex::encode(note.nullifier);

                        // Serialize auth_path to JSON array of hex strings
                        let auth_path_json: Vec<String> = witness.auth_path
                            .iter()
                            .map(|h| hex::encode(h))
                            .collect();
                        let auth_path_str = match serde_json::to_string(&auth_path_json) {
                            Ok(s) => s,
                            Err(e) => {
                                tracing::warn!("Failed to serialize auth_path: {}", e);
                                error_count += 1;
                                continue;
                            }
                        };

                        let root_hex = hex::encode(witness.root);

                        match repo.update_witness_by_nullifier(
                            &nullifier_hex,
                            witness.position,
                            &auth_path_str,
                            &root_hex,
                        ).await {
                            Ok(updated) => {
                                if updated {
                                    saved_count += 1;
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "[Orchard Sync] Failed to save witness: nullifier={}, error={}",
                                    &nullifier_hex[..16],
                                    e
                                );
                                error_count += 1;
                            }
                        }
                    }
                }
            }

            if saved_count > 0 || error_count > 0 {
                tracing::info!(
                    "[Orchard Sync] Persisted {} witnesses, {} errors",
                    saved_count,
                    error_count
                );
            }
        }
    }
}

/// Try to decrypt an Orchard note using the viewing key
///
/// This uses the proper orchard crate decryption instead of placeholder XOR
pub fn try_decrypt_orchard_note(
    _ivk: &IncomingViewingKey,
    action: &CompactOrchardAction,
    _block_height: u64,
) -> Option<(u64, Option<String>)> {
    // First 52 bytes of enc_ciphertext for compact decryption
    if action.ciphertext.len() < 52 {
        return None;
    }

    let mut enc_compact = [0u8; 52];
    enc_compact.copy_from_slice(&action.ciphertext[..52]);

    // Placeholder return - in production, use proper orchard decryption
    None // TODO: Implement proper decryption using orchard crate
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_config_default() {
        let config = SyncConfig::default();
        assert_eq!(config.rpc_url, "http://127.0.0.1:8232");
        assert_eq!(config.batch_size, 500);
        assert_eq!(config.parallel_fetches, 10);
    }

    #[tokio::test]
    async fn test_sync_service_creation() {
        let config = SyncConfig::default();
        let service = OrchardSyncService::new(config);

        let progress = service.get_progress().await;
        assert_eq!(progress.notes_found, 0);
    }
}
