#![allow(dead_code)]

use std::sync::Arc;
use tokio::sync::RwLock;
use sqlx::MySqlPool;

use crate::blockchain::zcash::orchard::{
    keys::OrchardKeyManager,
    scanner::ShieldedBalance,
    transfer::{FundSource, NetworkType, OrchardTransferService, TransferProposal, TransferResult},
    witness_sync::WitnessSyncManager,
    ScanProgress, ShieldedPool, UnifiedAddressInfo,
};
use crate::blockchain::ChainRegistry;
use crate::config::SecurityConfig;
use crate::crypto::{
    decrypt, encrypt, generate_ethereum_wallet, generate_zcash_wallet,
    import_ethereum_wallet, import_zcash_wallet,
};
use crate::crypto::zcash::{
    enable_orchard_for_wallet, generate_unified_address, is_unified_address, parse_unified_address,
};
use crate::db::models::{BalanceResponse, TokenBalance, Wallet, WalletResponse};
use crate::db::repositories::WalletRepository;
use crate::error::{AppError, AppResult};

pub struct WalletService {
    wallet_repo: WalletRepository,
    chain_registry: Arc<ChainRegistry>,
    security_config: SecurityConfig,
    /// Witness sync manager for Orchard shielded transactions
    witness_sync: Arc<RwLock<Option<WitnessSyncManager>>>,
    /// Database pool for persistence
    db_pool: MySqlPool,
    /// Transfer repository for recording transfers
    transfer_repo: crate::db::repositories::TransferRepository,
}

impl WalletService {
    pub fn new(
        wallet_repo: WalletRepository,
        chain_registry: Arc<ChainRegistry>,
        security_config: SecurityConfig,
        db_pool: MySqlPool,
    ) -> Self {
        let transfer_repo = crate::db::repositories::TransferRepository::new(db_pool.clone());
        Self {
            wallet_repo,
            chain_registry,
            security_config,
            witness_sync: Arc::new(RwLock::new(None)),
            db_pool,
            transfer_repo,
        }
    }

    /// Initialize Orchard witness sync manager with RPC configuration and database persistence
    pub async fn init_orchard_sync(&self, rpc_url: &str, rpc_user: Option<&str>, rpc_password: Option<&str>) -> AppResult<()> {
        let db_repo = Arc::new(crate::db::repositories::OrchardRepository::new(self.db_pool.clone()));

        // Create witness sync manager
        let witness_manager = WitnessSyncManager::new(
            db_repo,
            rpc_url.to_string(),
            rpc_user.unwrap_or("").to_string(),
            rpc_password.unwrap_or("").to_string(),
        );

        // Register all existing Zcash wallets with Orchard enabled
        let wallets = self.wallet_repo.list_all().await?;
        for wallet in wallets {
            if wallet.chain == "zcash" {
                if let Ok(vk) = self.get_viewing_key_for_wallet(&wallet).await {
                    witness_manager.register_wallet(wallet.id, vk).await;
                }
            }
        }

        // Initialize from saved state or frontier
        match witness_manager.initialize().await {
            Ok(height) => {
                if height > 0 {
                    tracing::info!("[Orchard Sync] Restored state from height {}", height);
                } else {
                    tracing::info!("[Orchard Sync] No saved state, will initialize on first sync");
                }
            }
            Err(e) => {
                tracing::warn!("[Orchard Sync] Failed to initialize: {}", e);
            }
        }

        let mut witness_sync = self.witness_sync.write().await;
        *witness_sync = Some(witness_manager);

        tracing::info!("Orchard witness sync manager initialized");
        Ok(())
    }

    /// Get viewing key for a wallet
    async fn get_viewing_key_for_wallet(&self, wallet: &Wallet) -> AppResult<crate::blockchain::zcash::orchard::OrchardViewingKey> {
        let private_key = decrypt(
            &wallet.encrypted_private_key,
            &self.security_config.encryption_key,
        )?;

        // Use stored birthday_height, fallback to Orchard activation height if not set
        let birthday_height = match wallet.orchard_birthday_height {
            Some(h) => h,
            None => self.orchard_activation_height().await,
        };

        let (_, viewing_key) = OrchardKeyManager::derive_from_private_key(&private_key, 0, birthday_height)
            .map_err(|e| AppError::InternalError(format!("Failed to derive viewing key: {}", e)))?;

        Ok(viewing_key)
    }

    /// Resolve the shielded (Zcash) consensus network from the live node so
    /// generated addresses carry the right HRP (t1/u1 mainnet vs tm/uregtest
    /// testnet/regtest). Falls back to mainnet if the zcash client or RPC is
    /// unavailable.
    async fn shielded_network(&self) -> zcash_protocol::consensus::NetworkType {
        let kind = match self.chain_registry.get("zcash") {
            Ok(c) => c
                .get_shielded_network()
                .await
                .unwrap_or_else(|_| "main".to_string()),
            Err(_) => "main".to_string(),
        };
        crate::crypto::zcash::consensus_network_from_kind(&kind)
    }

    /// Orchard (NU5) activation height read live from the node — the scan
    /// birthday / frontier floor. Falls back to the mainnet constant when the
    /// node doesn't expose it.
    async fn orchard_activation_height(&self) -> u64 {
        match self.chain_registry.get("zcash") {
            Ok(c) => c.get_shielded_activation_height().await.unwrap_or(1_687_104),
            Err(_) => 1_687_104,
        }
    }

    /// Create a new wallet with generated private key
    pub async fn create_wallet(&self, name: &str, chain: &str) -> AppResult<WalletResponse> {
        // Verify chain is supported
        let chain_client = self.chain_registry.get(chain)?;

        // Generate wallet based on chain type
        let (address, private_key) = match chain {
            "zcash" => {
                // Derive the address for whatever network this node runs
                // (mainnet t1/u1 vs regtest/testnet tm/uregtest).
                let net_kind = chain_client
                    .get_shielded_network()
                    .await
                    .unwrap_or_else(|_| "main".to_string());
                let network = crate::crypto::zcash::consensus_network_from_kind(&net_kind);
                generate_zcash_wallet(network)?
            }
            "ethereum" | _ => generate_ethereum_wallet()?,
        };

        // Check if address already exists
        if self.wallet_repo.find_by_address(&address, chain).await?.is_some() {
            return Err(AppError::AlreadyExists(format!(
                "Wallet with address {} already exists",
                address
            )));
        }

        // For Zcash wallets, get current block height as birthday
        let orchard_birthday_height = if chain == "zcash" {
            match chain_client.get_block_height().await {
                Ok(height) => {
                    tracing::info!("New Zcash wallet birthday_height set to {}", height);
                    Some(height)
                }
                Err(e) => {
                    tracing::warn!("Failed to get block height for birthday, using None: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Encrypt private key
        let encrypted_key = encrypt(&private_key, &self.security_config.encryption_key)?;

        // Store wallet with birthday height
        let id = self
            .wallet_repo
            .create(name, &address, &encrypted_key, chain, orchard_birthday_height)
            .await?;

        // Import address into chain node for tracking (needed for UTXO-based chains like Zcash)
        if let Err(e) = chain_client.import_address_for_tracking(&address, name).await {
            tracing::warn!("Failed to import address for tracking: {}", e);
            // Don't fail wallet creation, just warn
        }

        let wallet = self
            .wallet_repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| AppError::InternalError("Failed to retrieve created wallet".to_string()))?;

        Ok(WalletResponse::from(wallet))
    }

    /// Import an existing wallet from private key
    pub async fn import_wallet(
        &self,
        name: &str,
        private_key: &str,
        chain: &str,
    ) -> AppResult<WalletResponse> {
        // Verify chain is supported
        let chain_client = self.chain_registry.get(chain)?;

        // Parse and validate private key based on chain type
        let key = private_key.strip_prefix("0x").unwrap_or(private_key);

        let address = match chain {
            "zcash" => {
                let net_kind = chain_client
                    .get_shielded_network()
                    .await
                    .unwrap_or_else(|_| "main".to_string());
                let network = crate::crypto::zcash::consensus_network_from_kind(&net_kind);
                import_zcash_wallet(key, network)?
            }
            "ethereum" | _ => import_ethereum_wallet(key)?,
        };

        // Check if address already exists
        if self.wallet_repo.find_by_address(&address, chain).await?.is_some() {
            return Err(AppError::AlreadyExists(format!(
                "Wallet with address {} already exists",
                address
            )));
        }

        // For Zcash wallets, get current block height as birthday
        // Note: For imported wallets, user may want to set an earlier birthday to scan historical transactions
        let orchard_birthday_height = if chain == "zcash" {
            match chain_client.get_block_height().await {
                Ok(height) => {
                    tracing::info!("Imported Zcash wallet birthday_height set to {}", height);
                    Some(height)
                }
                Err(e) => {
                    tracing::warn!("Failed to get block height for birthday, using None: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // Encrypt private key
        let encrypted_key = encrypt(key, &self.security_config.encryption_key)?;

        // Store wallet with birthday height
        let id = self
            .wallet_repo
            .create(name, &address, &encrypted_key, chain, orchard_birthday_height)
            .await?;

        // Import address into chain node for tracking (needed for UTXO-based chains like Zcash)
        if let Err(e) = chain_client.import_address_for_tracking(&address, name).await {
            tracing::warn!("Failed to import address for tracking: {}", e);
            // Don't fail wallet import, just warn
        }

        let wallet = self
            .wallet_repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| AppError::InternalError("Failed to retrieve imported wallet".to_string()))?;

        Ok(WalletResponse::from(wallet))
    }

    /// List all wallets
    pub async fn list_wallets(&self) -> AppResult<Vec<WalletResponse>> {
        let wallets = self.wallet_repo.list_all().await?;
        Ok(wallets.into_iter().map(WalletResponse::from).collect())
    }

    /// List wallets by chain
    pub async fn list_wallets_by_chain(&self, chain: &str) -> AppResult<Vec<WalletResponse>> {
        let wallets = self.wallet_repo.list_by_chain(chain).await?;
        Ok(wallets.into_iter().map(WalletResponse::from).collect())
    }

    /// Get wallet by ID
    pub async fn get_wallet(&self, id: i32) -> AppResult<WalletResponse> {
        let wallet = self
            .wallet_repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound("Wallet not found".to_string()))?;

        Ok(WalletResponse::from(wallet))
    }

    /// Get active wallet for a chain
    pub async fn get_active_wallet(&self, chain: &str) -> AppResult<Wallet> {
        self.wallet_repo
            .get_active_wallet(chain)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("No active wallet for chain {}", chain)))
    }

    /// Set a wallet as active
    pub async fn set_active_wallet(&self, id: i32) -> AppResult<()> {
        let wallet = self
            .wallet_repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound("Wallet not found".to_string()))?;

        self.wallet_repo.set_active(id, &wallet.chain).await
    }

    /// Get wallet balance
    pub async fn get_balance(&self, address: &str, chain: &str) -> AppResult<BalanceResponse> {
        let chain_client = self.chain_registry.get(chain)?;

        let (native_balance, token_balances) = chain_client.get_all_balances(address).await?;

        Ok(BalanceResponse {
            address: address.to_string(),
            chain: chain.to_string(),
            native_balance: native_balance.to_string(),
            tokens: token_balances
                .into_iter()
                .map(|t| TokenBalance {
                    symbol: t.symbol,
                    balance: t.balance.to_string(),
                    contract_address: t.contract_address,
                })
                .collect(),
        })
    }

    /// Export private key (requires password verification)
    pub async fn export_private_key(&self, wallet_id: i32) -> AppResult<String> {
        let wallet = self
            .wallet_repo
            .find_by_id(wallet_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Wallet not found".to_string()))?;

        let private_key = decrypt(
            &wallet.encrypted_private_key,
            &self.security_config.encryption_key,
        )?;

        Ok(format!("0x{}", private_key))
    }

    /// Get decrypted private key for internal use
    pub async fn get_private_key(&self, wallet_id: i32) -> AppResult<String> {
        let wallet = self
            .wallet_repo
            .find_by_id(wallet_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Wallet not found".to_string()))?;

        decrypt(
            &wallet.encrypted_private_key,
            &self.security_config.encryption_key,
        )
    }

    /// Delete a wallet
    pub async fn delete_wallet(&self, id: i32) -> AppResult<()> {
        // Verify wallet exists
        self.wallet_repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound("Wallet not found".to_string()))?;

        self.wallet_repo.delete(id).await
    }

    // =========================================================================
    // Orchard Privacy Protocol Methods
    // =========================================================================

    /// Enable Orchard (shielded) functionality for a Zcash wallet
    ///
    /// This derives Orchard keys from the existing transparent private key
    /// and generates a unified address that can receive both transparent and shielded funds.
    ///
    /// # Arguments
    /// * `wallet_id` - ID of the Zcash wallet to enable Orchard for
    /// * `birthday_height` - Block height when the wallet was created (for scanning)
    ///
    /// # Returns
    /// * Unified address info and encoded viewing key
    pub async fn enable_orchard(
        &self,
        wallet_id: i32,
        birthday_height: u64,
    ) -> AppResult<(UnifiedAddressInfo, String)> {
        // Get wallet
        let wallet = self
            .wallet_repo
            .find_by_id(wallet_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Wallet not found".to_string()))?;

        // Verify it's a Zcash wallet
        if wallet.chain != "zcash" {
            return Err(AppError::ValidationError(
                "Orchard is only available for Zcash wallets".to_string(),
            ));
        }

        // Decrypt private key
        let private_key = decrypt(
            &wallet.encrypted_private_key,
            &self.security_config.encryption_key,
        )?;

        // Enable Orchard and get unified address (network-aware HRP)
        let network = self.shielded_network().await;
        let (unified_address, viewing_key_encoded) =
            enable_orchard_for_wallet(&private_key, birthday_height, network)?;

        // TODO: Initialize Orchard scanner for background block scanning
        // The scanner is optional and used for discovering incoming shielded transactions.
        // For now, we skip this step as it requires running a lightwalletd instance.
        // Users can still send shielded transactions without the scanner.

        tracing::info!(
            "Enabled Orchard for wallet {}, unified address: {}",
            wallet_id,
            unified_address.address
        );

        Ok((unified_address, viewing_key_encoded))
    }

    /// Get all unified addresses for a wallet
    ///
    /// This regenerates the unified address from the private key (deterministic).
    /// In a production system, addresses would be stored in a database.
    ///
    /// # Arguments
    /// * `wallet_id` - ID of the wallet
    ///
    /// # Returns
    /// * List of unified addresses
    pub async fn get_unified_addresses(
        &self,
        wallet_id: i32,
    ) -> AppResult<Vec<UnifiedAddressInfo>> {
        let wallet = self
            .wallet_repo
            .find_by_id(wallet_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Wallet not found".to_string()))?;

        if wallet.chain != "zcash" {
            return Err(AppError::ValidationError(
                "Unified addresses only available for Zcash wallets".to_string(),
            ));
        }

        // Decrypt private key
        let private_key = decrypt(
            &wallet.encrypted_private_key,
            &self.security_config.encryption_key,
        )?;

        // Use stored birthday_height, fallback to Orchard activation height
        let birthday_height = match wallet.orchard_birthday_height {
            Some(h) => h,
            None => self.orchard_activation_height().await,
        };

        // Try to regenerate the unified address (deterministic from private key)
        let network = self.shielded_network().await;
        match enable_orchard_for_wallet(&private_key, birthday_height, network) {
            Ok((unified_address, _viewing_key)) => Ok(vec![unified_address]),
            Err(_) => {
                // Orchard not enabled or error - return empty list
                Ok(vec![])
            }
        }
    }

    /// Generate a new unified address for a wallet that has Orchard enabled
    ///
    /// # Arguments
    /// * `viewing_key_encoded` - The encoded viewing key from enable_orchard
    /// * `address_index` - Index for the new address (0 for first, incrementing)
    ///
    /// # Returns
    /// * New unified address info
    pub async fn generate_new_unified_address(
        &self,
        viewing_key_encoded: &str,
        address_index: u32,
    ) -> AppResult<UnifiedAddressInfo> {
        let network = self.shielded_network().await;
        generate_unified_address(viewing_key_encoded, address_index, network)
    }

    /// Get shielded (Orchard) balance for a wallet
    ///
    /// This automatically syncs with the blockchain before returning the balance.
    ///
    /// # Arguments
    /// * `wallet_id` - ID of the wallet (must have Orchard enabled)
    ///
    /// # Returns
    /// * Shielded balance breakdown
    pub async fn get_shielded_balance(&self, wallet_id: i32) -> AppResult<ShieldedBalance> {
        let wallet = self
            .wallet_repo
            .find_by_id(wallet_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Wallet not found".to_string()))?;

        if wallet.chain != "zcash" {
            return Err(AppError::ValidationError(
                "Shielded balance only available for Zcash wallets".to_string(),
            ));
        }

        // Ensure sync service is initialized (don't sync here - it's too slow)
        // Sync happens in background task or via explicit /sync endpoint
        self.ensure_orchard_sync_initialized().await?;

        // Get balance from witness sync manager (fast, from database)
        let witness_sync = self.witness_sync.read().await;
        if let Some(manager) = witness_sync.as_ref() {
            let balance = manager.get_wallet_balance(wallet_id).await;
            return Ok(balance);
        }

        // Fallback to zero if sync not initialized
        Ok(ShieldedBalance::new(ShieldedPool::Orchard, 0, 0, 0))
    }

    /// Get unspent notes from database
    pub async fn get_unspent_notes_from_db(&self, wallet_id: i32) -> AppResult<Vec<crate::db::repositories::orchard_repo::StoredOrchardNote>> {
        let wallet = self
            .wallet_repo
            .find_by_id(wallet_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Wallet not found".to_string()))?;

        if wallet.chain != "zcash" {
            return Err(AppError::ValidationError(
                "Notes only available for Zcash wallets".to_string(),
            ));
        }

        let repo = crate::db::repositories::OrchardRepository::new(self.db_pool.clone());
        repo.get_unspent_notes(wallet_id).await
    }

    /// Ensure Orchard sync service is initialized
    async fn ensure_orchard_sync_initialized(&self) -> AppResult<()> {
        // Check if already initialized
        {
            let witness_sync = self.witness_sync.read().await;
            if witness_sync.is_some() {
                return Ok(());
            }
        }

        // Initialize sync service
        tracing::info!("Auto-initializing Orchard witness sync manager");

        let chain_client = self.chain_registry.get("zcash")?;
        let rpc_url = chain_client.get_rpc_url().await
            .ok_or_else(|| AppError::InternalError("No RPC URL configured for Zcash".to_string()))?;
        let rpc_auth = chain_client.get_rpc_auth().await;

        let (rpc_user, rpc_password) = match rpc_auth {
            Some((u, p)) => (Some(u), Some(p)),
            None => (None, None),
        };

        self.init_orchard_sync(&rpc_url, rpc_user.as_deref(), rpc_password.as_deref()).await
    }

    /// Internal sync method that doesn't re-initialize
    async fn sync_orchard_internal(&self) -> AppResult<ScanProgress> {
        let witness_sync = self.witness_sync.read().await;

        if let Some(manager) = witness_sync.as_ref() {
            // Get chain height and sync state
            let chain_tip = manager.get_chain_height().await
                .map_err(|e| AppError::BlockchainError(format!("Failed to get chain height: {}", e)))?;
            let mut tree_height = manager.get_tree_height().await;

            // Check if there are notes without witness_state that need rescanning
            if let Some(rescan_from_height) = manager.check_notes_need_rescan().await
                .map_err(|e| AppError::BlockchainError(format!("Failed to check notes: {}", e)))? {
                tracing::warn!(
                    "[Orchard Sync] Notes without witness_state found. Resetting tree to rescan from block {}",
                    rescan_from_height
                );

                // Reset tree and reinitialize from the note's block height
                manager.reset_for_rescan(rescan_from_height).await
                    .map_err(|e| AppError::BlockchainError(format!("Failed to reset for rescan: {}", e)))?;

                tree_height = manager.get_tree_height().await;
            }

            // If tree is not initialized (height=0), initialize from frontier
            if tree_height == 0 {
                // Find the earliest note's block_height, or use Orchard activation height
                let min_height = manager.get_min_scan_height().await
                    .map_err(|e| AppError::BlockchainError(format!("Failed to get min height: {}", e)))?;

                // Initialize from frontier at height-1 (to include notes in that
                // block), floored at the network's Orchard activation height so
                // short regtest/testnet chains don't request a nonexistent block.
                let activation = self.orchard_activation_height().await;
                let frontier_height = min_height.saturating_sub(1).max(activation);
                tracing::info!(
                    "[Orchard Sync] Initializing tree from frontier at height {}",
                    frontier_height
                );

                manager.init_from_frontier(frontier_height).await
                    .map_err(|e| AppError::BlockchainError(format!("Failed to init from frontier: {}", e)))?;

                tree_height = frontier_height;
            }

            // If tree is behind, we need to sync
            if tree_height < chain_tip {
                tracing::info!(
                    "[Orchard Sync] Syncing from {} to {}",
                    tree_height,
                    chain_tip
                );

                // Get known positions for existing notes
                let known_positions = manager.build_known_positions_map().await
                    .map_err(|e| AppError::BlockchainError(format!("Failed to build positions: {}", e)))?;

                // Fetch blocks in batches and process
                let mut current = tree_height + 1;
                let batch_size = 500u64;

                while current <= chain_tip {
                    let end = std::cmp::min(current + batch_size - 1, chain_tip);

                    // Fetch and process blocks
                    let blocks = manager.fetch_blocks(current, end).await
                        .map_err(|e| AppError::BlockchainError(format!("Failed to fetch blocks: {}", e)))?;

                    if !blocks.is_empty() {
                        let found_notes = manager.process_blocks(blocks, &known_positions).await
                            .map_err(|e| AppError::BlockchainError(format!("Failed to process blocks: {}", e)))?;

                        // Save any newly found notes
                        if !found_notes.is_empty() {
                            tracing::info!(
                                "[Orchard Sync] Found {} new notes in blocks {}-{}",
                                found_notes.len(),
                                current,
                                end
                            );
                            manager.save_notes(&found_notes).await
                                .map_err(|e| AppError::BlockchainError(format!("Failed to save notes: {}", e)))?;
                        }
                    }

                    current = end + 1;
                }

                // Save state after sync
                manager.save_state().await
                    .map_err(|e| AppError::BlockchainError(format!("Failed to save state: {}", e)))?;

                // Update sync state for all wallets
                let wallet_ids = manager.get_wallet_ids().await;
                for wallet_id in wallet_ids {
                    let _ = manager.update_sync_state(wallet_id, chain_tip).await;
                }
            }

            Ok(manager.get_progress().await)
        } else {
            Err(AppError::InternalError("Orchard sync not initialized".to_string()))
        }
    }

    /// Get combined balance (transparent + shielded) for a Zcash wallet
    ///
    /// # Arguments
    /// * `wallet_id` - ID of the wallet
    ///
    /// # Returns
    /// * Balance response with both transparent and shielded balances
    pub async fn get_combined_zcash_balance(
        &self,
        wallet_id: i32,
    ) -> AppResult<CombinedZcashBalance> {
        let wallet = self
            .wallet_repo
            .find_by_id(wallet_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Wallet not found".to_string()))?;

        if wallet.chain != "zcash" {
            return Err(AppError::ValidationError(
                "This endpoint is only for Zcash wallets".to_string(),
            ));
        }

        let chain_client = self.chain_registry.get("zcash")?;

        // Get transparent balance
        let transparent_balance = chain_client.get_native_balance(&wallet.address).await?;

        // Try to get shielded balance (may fail if Orchard not enabled)
        let shielded_balance = match self.get_shielded_balance(wallet_id).await {
            Ok(balance) => Some(balance),
            Err(_) => None,
        };

        let total_zatoshis = (transparent_balance
            .to_string()
            .parse::<f64>()
            .unwrap_or(0.0)
            * 100_000_000.0) as u64
            + shielded_balance
                .as_ref()
                .map(|b| b.total_zatoshis)
                .unwrap_or(0);

        Ok(CombinedZcashBalance {
            wallet_id,
            address: wallet.address,
            transparent_balance: transparent_balance.to_string(),
            shielded_balance,
            total_zec: total_zatoshis as f64 / 100_000_000.0,
        })
    }

    /// Trigger Orchard sync - scans blockchain for shielded transactions
    pub async fn sync_orchard(&self) -> AppResult<ScanProgress> {
        // Ensure sync is initialized
        self.ensure_orchard_sync_initialized().await?;

        tracing::info!("Starting Orchard blockchain sync");

        let progress = self.sync_orchard_internal().await?;

        tracing::info!(
            "Orchard sync complete: {} blocks scanned, {} notes found",
            progress.last_scanned_height,
            progress.notes_found
        );

        Ok(progress)
    }

    /// Get current Orchard scan progress
    pub async fn get_scan_progress(&self) -> AppResult<ScanProgress> {
        let witness_sync = self.witness_sync.read().await;

        if let Some(manager) = witness_sync.as_ref() {
            Ok(manager.get_progress().await)
        } else {
            // Return default progress if not initialized
            let chain_tip = self.chain_registry.get("zcash")
                .ok()
                .map(|c| futures::executor::block_on(c.get_block_height()).unwrap_or(2_500_000))
                .unwrap_or(2_500_000);

            // Use the network's Orchard activation height as default start
            let activation = self.orchard_activation_height().await;
            Ok(ScanProgress::new("zcash", "orchard", activation, chain_tip))
        }
    }

    /// Parse a unified address
    pub fn parse_address(&self, address: &str) -> AppResult<UnifiedAddressInfo> {
        parse_unified_address(address)
    }

    /// Check if an address is a unified address
    pub fn is_unified(&self, address: &str) -> bool {
        is_unified_address(address)
    }

    /// Create a privacy transfer proposal
    ///
    /// This validates the transfer request and creates a proposal without building
    /// the actual transaction. The proposal includes fee estimation and validation.
    ///
    /// # Arguments
    /// * `wallet_id` - Source wallet ID
    /// * `to_address` - Recipient address (unified or transparent)
    /// * `amount_zec` - Amount in ZEC
    /// * `memo` - Optional encrypted memo
    /// * `fund_source` - Source of funds (auto, shielded, or transparent)
    ///
    /// # Returns
    /// * Transfer proposal with fee estimation
    pub async fn create_privacy_transfer_proposal(
        &self,
        wallet_id: i32,
        to_address: &str,
        amount_zec: &str,
        amount_zatoshis: Option<u64>,
        memo: Option<String>,
        fund_source: FundSource,
    ) -> AppResult<TransferProposal> {
        let wallet = self
            .wallet_repo
            .find_by_id(wallet_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Wallet not found".to_string()))?;

        if wallet.chain != "zcash" {
            return Err(AppError::ValidationError(
                "Privacy transfers are only available for Zcash wallets".to_string(),
            ));
        }

        // Get balances
        let chain_client = self.chain_registry.get("zcash")?;
        let transparent_balance = chain_client.get_native_balance(&wallet.address).await?;
        let transparent_zatoshis = (transparent_balance
            .to_string()
            .parse::<f64>()
            .unwrap_or(0.0)
            * 100_000_000.0) as u64;

        let shielded_balance = self.get_shielded_balance(wallet_id).await.ok();

        // Get current block height
        let current_height = chain_client.get_block_height().await.unwrap_or(2_500_000);

        // Create transfer service and proposal
        let transfer_service = OrchardTransferService::new(NetworkType::Mainnet);

        let request = crate::blockchain::zcash::orchard::transfer::TransferRequest {
            wallet_id,
            to_address: to_address.to_string(),
            amount_zec: amount_zec.to_string(),
            amount_zatoshis, // Pass through the zatoshis if provided
            memo,
            fund_source,
        };

        tracing::debug!(
            "Creating transfer proposal: amount_zec={}, amount_zatoshis={:?}",
            amount_zec,
            amount_zatoshis
        );

        transfer_service
            .create_proposal(
                &request,
                transparent_zatoshis,
                shielded_balance.as_ref(),
                current_height,
            )
            .map_err(|e| AppError::BlockchainError(e.to_string()))
    }

    /// Execute a privacy transfer
    ///
    /// This builds, signs, and broadcasts the transaction.
    ///
    /// # Arguments
    /// * `proposal` - The transfer proposal to execute
    ///
    /// # Returns
    /// * Transfer result with transaction ID
    pub async fn execute_privacy_transfer(
        &self,
        wallet_id: i32,
        proposal: &TransferProposal,
    ) -> AppResult<TransferResult> {
        use crate::blockchain::zcash::orchard::keys::OrchardKeyManager;

        // CRITICAL SAFETY CHECK: Prevent zero-value transactions
        if proposal.amount_zatoshis == 0 {
            tracing::error!(
                "BLOCKED in execute_privacy_transfer: amount_zatoshis=0! proposal={}, to={}",
                proposal.proposal_id,
                proposal.to_address
            );
            return Err(AppError::ValidationError(
                "Cannot execute transfer with zero amount".to_string(),
            ));
        }

        tracing::info!(
            "execute_privacy_transfer: wallet={}, proposal={}, amount={} zatoshis, fee={} zatoshis",
            wallet_id,
            proposal.proposal_id,
            proposal.amount_zatoshis,
            proposal.fee_zatoshis
        );

        let wallet = self
            .wallet_repo
            .find_by_id(wallet_id)
            .await?
            .ok_or_else(|| AppError::NotFound("Wallet not found".to_string()))?;

        if wallet.chain != "zcash" {
            return Err(AppError::ValidationError(
                "Privacy transfers are only available for Zcash wallets".to_string(),
            ));
        }

        // Decrypt private key
        let private_key = decrypt(
            &wallet.encrypted_private_key,
            &self.security_config.encryption_key,
        )?;

        // Use stored birthday_height, fallback to Orchard activation height
        let birthday_height = match wallet.orchard_birthday_height {
            Some(h) => h,
            None => self.orchard_activation_height().await,
        };

        // Derive Orchard spending key from private key
        let (spending_key, _viewing_key) =
            OrchardKeyManager::derive_from_private_key(&private_key, 0, birthday_height)
                .map_err(|e| AppError::InternalError(format!("Failed to derive keys: {}", e)))?;

        // Get chain client for UTXOs and broadcasting
        let chain_client = self.chain_registry.get("zcash")?;

        // Create transfer service pinned to the node's live consensus branch id
        // so the shielded-tx sighash matches the active network upgrade (NU6.2
        // testnet/regtest, mainnet NU6.1, future NU7…). Without this the node
        // rejects the broadcast with "incorrect consensus branch id" (-25).
        let consensus_branch_id = chain_client
            .get_consensus_branch_id()
            .await
            .map_err(|e| AppError::BlockchainError(format!(
                "Failed to read consensus branch id: {}", e
            )))?;
        let transfer_service =
            OrchardTransferService::new_with_branch_id(NetworkType::Mainnet, consensus_branch_id);

        // Get transparent inputs (UTXOs) for shielding
        // CRITICAL: Only select UTXOs needed to cover amount + fee, not ALL UTXOs!
        // Otherwise excess funds become miner fees (no change output in current implementation)
        let transparent_inputs = if proposal.fund_source == FundSource::Transparent
            || proposal.is_shielding
        {
            let mut utxos = chain_client.get_utxos(&wallet.address).await?;
            tracing::debug!("Found {} UTXOs for address {}", utxos.len(), wallet.address);

            // Sort UTXOs by value descending to minimize number of inputs
            utxos.sort_by(|a, b| b.value.cmp(&a.value));

            // Calculate total needed (amount + fee)
            let total_needed = proposal.amount_zatoshis + proposal.fee_zatoshis;
            tracing::info!(
                "Selecting UTXOs: need {} zatoshis (amount={} + fee={})",
                total_needed,
                proposal.amount_zatoshis,
                proposal.fee_zatoshis
            );

            // Select only UTXOs needed to cover the total
            let mut selected_utxos = Vec::new();
            let mut selected_total: u64 = 0;

            for utxo in utxos {
                if selected_total >= total_needed {
                    break; // We have enough
                }

                tracing::debug!(
                    "Selecting UTXO: txid={}, vout={}, value={} zatoshis",
                    utxo.txid,
                    utxo.output_index,
                    utxo.value
                );

                selected_total += utxo.value;

                // Parse txid - stored in big-endian for display
                let mut prev_tx_hash = [0u8; 32];
                if let Ok(bytes) = hex::decode(&utxo.txid) {
                    if bytes.len() == 32 {
                        prev_tx_hash.copy_from_slice(&bytes);
                    }
                }
                // This is the scriptPubKey (locking script), NOT the signature
                let script_pubkey = hex::decode(&utxo.script).unwrap_or_default();

                selected_utxos.push(crate::blockchain::zcash::orchard::transfer::TransparentInput {
                    prev_tx_hash,
                    prev_tx_index: utxo.output_index,
                    script_pubkey,
                    value: utxo.value,
                    sequence: 0xfffffffe, // Enable RBF
                });
            }

            // Recalculate fee based on actual input count
            // ZIP-317: fee = 5000 * max(2, transparent_inputs + orchard_actions)
            // Orchard actions = 2 (payment + change, padded to even)
            let num_inputs = selected_utxos.len() as u64;
            let orchard_actions: u64 = 2; // payment + change
            let logical_actions = num_inputs + orchard_actions;
            let actual_fee_needed = 5000 * std::cmp::max(2, logical_actions);

            tracing::info!(
                "ZIP-317 fee: inputs={}, orchard_actions={}, logical_actions={}, required_fee={}",
                num_inputs,
                orchard_actions,
                logical_actions,
                actual_fee_needed
            );

            // If actual fee > proposal fee, we need more funds
            let effective_fee = std::cmp::max(proposal.fee_zatoshis, actual_fee_needed);
            let total_needed_with_actual_fee = proposal.amount_zatoshis + effective_fee;

            // Check if we need to select more UTXOs due to higher fee
            if selected_total < total_needed_with_actual_fee {
                tracing::warn!(
                    "Need more funds due to higher fee: have={}, need={} (fee increased from {} to {})",
                    selected_total,
                    total_needed_with_actual_fee,
                    proposal.fee_zatoshis,
                    effective_fee
                );
                return Err(AppError::ValidationError(format!(
                    "Insufficient balance after fee adjustment: have {} zatoshis, need {} zatoshis (fee: {})",
                    selected_total, total_needed_with_actual_fee, effective_fee
                )));
            }

            // Calculate change (will be sent to shielded change address)
            let change_amount = selected_total - proposal.amount_zatoshis - effective_fee;

            tracing::info!(
                "Transaction breakdown: input={}, amount={}, fee={}, change={}",
                selected_total,
                proposal.amount_zatoshis,
                effective_fee,
                change_amount
            );

            // Update proposal fee if it increased (for accurate change calculation)
            // Note: We can't mutate proposal here, but the fee will be recalculated in build_transaction
            if effective_fee != proposal.fee_zatoshis {
                tracing::warn!(
                    "Fee adjusted from {} to {} zatoshis due to {} UTXOs",
                    proposal.fee_zatoshis,
                    effective_fee,
                    num_inputs
                );
            }

            selected_utxos
        } else {
            vec![]
        };

        // Get current block height for anchor
        let anchor_height = chain_client.get_block_height().await.unwrap_or(2_500_000);

        // Get spendable notes and anchor from the witness sync manager
        let (spendable_notes, tree_anchor, _tree_root) = if proposal.fund_source == FundSource::Shielded
            || proposal.fund_source == FundSource::Auto
        {
            // Always refresh witnesses to latest chain state before spending
            // This ensures auth_path and root are computed from the latest tree state
            {
                let sync_guard = self.witness_sync.read().await;
                if let Some(ref manager) = sync_guard.as_ref() {
                    let tree_height = manager.get_tree_height().await;
                    let chain_tip = manager.get_chain_height().await.unwrap_or(tree_height);

                    if tree_height < chain_tip {
                        tracing::info!(
                            "[Privacy Transfer] Refreshing witnesses: tree={} -> chain_tip={}",
                            tree_height,
                            chain_tip
                        );
                        // refresh_witnesses_for_spending updates tree and all witnesses
                        let _ = manager.refresh_witnesses_for_spending(wallet_id).await;
                    }
                }
            }

            // Get notes with witnesses
            let sync_guard = self.witness_sync.read().await;
            if let Some(manager) = sync_guard.as_ref() {
                let notes = manager.get_spendable_notes_with_witnesses(wallet_id).await;

                // Get anchor directly from the tree
                let anchor = manager.get_orchard_anchor().await;
                let tree_root = {
                    let tree = manager.tree().read().await;
                    tree.root()
                };

                tracing::info!(
                    "[Privacy Transfer] Tree anchor: {}",
                    hex::encode(&tree_root)
                );

                // Get MerklePath for each note directly using proper conversion
                let mut notes_with_paths: Vec<(crate::blockchain::zcash::orchard::scanner::OrchardNote, orchard::tree::MerklePath)> = Vec::new();

                for note in notes {
                    let nullifier_hex = hex::encode(&note.nullifier);
                    if let Some(merkle_path) = manager.get_orchard_merkle_path(&nullifier_hex).await {
                        tracing::debug!(
                            "[Privacy Transfer] Got MerklePath for note {}: position={}",
                            &nullifier_hex[..16],
                            note.position
                        );
                        notes_with_paths.push((note, merkle_path));
                    } else {
                        tracing::warn!(
                            "[Privacy Transfer] No MerklePath for note {}",
                            &nullifier_hex[..16]
                        );
                    }
                }

                tracing::info!(
                    "[Privacy Transfer] Loaded {} spendable notes with MerklePaths",
                    notes_with_paths.len()
                );

                if notes_with_paths.is_empty() && proposal.fund_source == FundSource::Shielded {
                    tracing::warn!(
                        "No spendable notes with witness data found. \
                         Notes may need to be rescanned to populate witnesses."
                    );
                }

                (notes_with_paths, anchor, tree_root)
            } else {
                tracing::warn!("Witness sync manager not initialized");
                (vec![], orchard::tree::Anchor::empty_tree(), [0u8; 32])
            }
        } else {
            (vec![], orchard::tree::Anchor::empty_tree(), [0u8; 32])
        };

        // Log for debugging (tree_root is just for logging)
        let _ = _tree_root;

        tracing::info!(
            "execute_privacy_transfer: fund_source={:?}, transparent_inputs={}, spendable_notes={}",
            proposal.fund_source,
            transparent_inputs.len(),
            spendable_notes.len()
        );

        // Build the Orchard transaction (includes proof generation and signing)
        // Now passes notes with their MerklePaths directly, and Anchor as proper type
        let result = transfer_service
            .build_transaction(
                proposal,
                &spending_key,
                &private_key, // Private key for signing transparent inputs
                spendable_notes,  // Vec<(OrchardNote, MerklePath)>
                transparent_inputs,
                anchor_height,
                tree_anchor,  // orchard::tree::Anchor
            )
            .map_err(|e| AppError::BlockchainError(format!("Failed to build transaction: {}", e)))?;

        // Broadcast using sendrawtransaction
        if let Some(ref raw_tx) = result.raw_tx {
            let tx_hash = chain_client
                .broadcast_raw_transaction(raw_tx)
                .await
                .map_err(|e| {
                    AppError::BlockchainError(format!("Failed to broadcast transaction: {}", e))
                })?;

            tracing::info!(
                "Privacy transfer broadcast successful: wallet={}, to={}, tx_hash={}",
                wallet_id,
                proposal.to_address,
                tx_hash
            );

            // Record the privacy transfer in database
            let amount_zec = rust_decimal::Decimal::from(proposal.amount_zatoshis)
                / rust_decimal::Decimal::from(100_000_000u64);
            let fee_zec = rust_decimal::Decimal::from(proposal.fee_zatoshis)
                / rust_decimal::Decimal::from(100_000_000u64);

            // Get unified address as from_address for shielded transfer
            let from_address = self.get_unified_addresses(wallet_id).await
                .ok()
                .and_then(|addrs| addrs.first().map(|a| a.address.clone()))
                .unwrap_or_else(|| wallet.address.clone());

            match self.transfer_repo.create(
                wallet_id,
                "zcash",
                &from_address,
                &proposal.to_address,
                "ZEC-shielded",  // Mark as shielded transfer
                amount_zec,
                Some(fee_zec),
                None,
                1,  // System initiated (TODO: pass actual user_id)
            ).await {
                Ok(transfer_id) => {
                    // Update status to submitted with tx_hash
                    if let Err(e) = self.transfer_repo.update_status(
                        transfer_id,
                        "submitted",
                        Some(&tx_hash),
                        None,
                    ).await {
                        tracing::warn!("Failed to update transfer status: {}", e);
                    }
                    tracing::info!(
                        "Privacy transfer recorded: id={}, wallet={}, amount={} ZEC",
                        transfer_id,
                        wallet_id,
                        amount_zec
                    );
                }
                Err(e) => {
                    tracing::warn!("Failed to record privacy transfer: {}", e);
                }
            }

            return Ok(TransferResult {
                tx_id: tx_hash,
                status: crate::blockchain::zcash::orchard::transfer::TransferStatus::Submitted,
                raw_tx: result.raw_tx,
                amount_zatoshis: proposal.amount_zatoshis,
                fee_zatoshis: proposal.fee_zatoshis,
            });
        }

        Ok(result)
    }

    /// Start background Orchard sync task
    ///
    /// This spawns a background task that syncs all Zcash wallets every 5 minutes.
    /// Should be called once at application startup.
    pub fn start_background_sync(self: Arc<Self>) {
        let service = self.clone();

        tokio::spawn(async move {
            // Wait 30 seconds before first sync to allow system to fully start
            tracing::info!("[Background Sync] Waiting 30s before first sync...");
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;

            tracing::info!("[Background Sync] ▶️ Starting Orchard background sync task (interval: 5 minutes)");

            let mut sync_count = 0u64;
            loop {
                sync_count += 1;
                tracing::info!("[Background Sync] ═══════════════════════════════════════════════════");
                tracing::info!("[Background Sync] Starting sync cycle #{}", sync_count);

                // Perform sync for all Zcash wallets
                let start_time = std::time::Instant::now();
                match service.sync_all_zcash_wallets().await {
                    Ok(count) => {
                        let elapsed = start_time.elapsed().as_secs_f64();
                        tracing::info!(
                            "[Background Sync] ✅ Sync cycle #{} completed: {} wallets synced in {:.1}s",
                            sync_count,
                            count,
                            elapsed
                        );
                    }
                    Err(e) => {
                        tracing::error!("[Background Sync] ❌ Sync cycle #{} failed: {}", sync_count, e);
                    }
                }

                tracing::info!("[Background Sync] Next sync in 5 minutes...");
                tracing::info!("[Background Sync] ═══════════════════════════════════════════════════");

                // Wait 1 minute before next sync
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            }
        });
    }

    /// Sync all Zcash wallets
    async fn sync_all_zcash_wallets(&self) -> AppResult<usize> {
        tracing::debug!("[Wallet Sync] Ensuring Orchard sync service is initialized...");

        // Ensure sync service is initialized
        self.ensure_orchard_sync_initialized().await?;

        // Get all Zcash wallets
        let wallets = self.wallet_repo.list_by_chain("zcash").await?;
        let wallet_count = wallets.len();

        if wallet_count == 0 {
            tracing::info!("[Wallet Sync] No Zcash wallets found in database");
            return Ok(0);
        }

        tracing::info!("[Wallet Sync] Found {} Zcash wallet(s) to sync", wallet_count);

        // Register all wallets with the witness sync manager
        let mut registered_count = 0usize;
        let mut failed_count = 0usize;
        {
            let witness_sync = self.witness_sync.read().await;
            if let Some(manager) = witness_sync.as_ref() {
                for wallet in &wallets {
                    match self.get_viewing_key_for_wallet(wallet).await {
                        Ok(vk) => {
                            manager.register_wallet(wallet.id, vk).await;
                            registered_count += 1;
                            tracing::debug!(
                                "[Wallet Sync] Registered wallet {} (address: {})",
                                wallet.id,
                                &wallet.address[..20]
                            );
                        }
                        Err(e) => {
                            failed_count += 1;
                            tracing::warn!(
                                "[Wallet Sync] Failed to get viewing key for wallet {}: {}",
                                wallet.id,
                                e
                            );
                        }
                    }
                }
            } else {
                tracing::error!("[Wallet Sync] Witness sync manager not available!");
            }
        }

        tracing::info!(
            "[Wallet Sync] Registered {}/{} wallets ({} failed)",
            registered_count,
            wallet_count,
            failed_count
        );

        // Perform sync (this also updates all witnesses)
        tracing::info!("[Wallet Sync] Starting blockchain scan...");
        match self.sync_orchard_internal().await {
            Ok(progress) => {
                tracing::info!(
                    "[Wallet Sync] Scan result: {:.1}% complete, scanned to block {}, {} notes found",
                    progress.progress_percent,
                    progress.last_scanned_height,
                    progress.notes_found
                );
            }
            Err(e) => {
                tracing::warn!("[Wallet Sync] Scan error: {}", e);
            }
        }

        Ok(wallet_count)
    }

    /// F4 — confirmation lookup for the run executor's double-spend gate.
    /// The executor must not build a new shielded spend for a wallet while
    /// an earlier broadcast is still unmined: the note DB only learns about
    /// a spend once the containing block is scanned, so spending again
    /// before then re-selects the same notes (duplicate nullifier, RPC -25).
    pub async fn zcash_tx_status(
        &self,
        tx_hash: &str,
    ) -> AppResult<crate::blockchain::traits::TxStatus> {
        let chain_client = self.chain_registry.get("zcash")?;
        chain_client.get_tx_status(tx_hash).await
    }
}

/// Combined balance for Zcash (transparent + shielded)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CombinedZcashBalance {
    pub wallet_id: i32,
    pub address: String,
    pub transparent_balance: String,
    pub shielded_balance: Option<ShieldedBalance>,
    pub total_zec: f64,
}
