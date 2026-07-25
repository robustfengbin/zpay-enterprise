//! Orchard Privacy Protocol API Handlers
//!
//! Handles API requests for Zcash Orchard shielded transfers.

use actix_web::{web, HttpResponse};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::api::middleware::AuthenticatedUser;
use crate::error::{AppError, AppResult};
use crate::services::WalletService;

/// Request to enable Orchard for a wallet
#[derive(Debug, Deserialize)]
pub struct EnableOrchardRequest {
    pub birthday_height: u64,
}

/// Response after enabling Orchard
#[derive(Debug, Serialize)]
pub struct EnableOrchardResponse {
    pub unified_address: UnifiedAddressInfo,
    pub viewing_key: String,
}

/// Unified address information
#[derive(Debug, Serialize)]
pub struct UnifiedAddressInfo {
    pub address: String,
    pub has_orchard: bool,
    pub has_sapling: bool,
    pub has_transparent: bool,
    pub transparent_address: Option<String>,
    pub address_index: u32,
    pub account_index: u32,
}

/// Shielded balance response
#[derive(Debug, Serialize)]
pub struct ShieldedBalanceResponse {
    pub total_zatoshis: u64,
    pub spendable_zatoshis: u64,
    pub pending_zatoshis: u64,
    pub note_count: u32,
    /// Pool name, omitted when the figure is the total across pools.
    pub pool: Option<String>,
}

/// Combined balance response
#[derive(Debug, Serialize)]
pub struct CombinedBalanceResponse {
    pub wallet_id: i32,
    pub address: String,
    pub transparent_balance: String,
    pub shielded_balance: Option<ShieldedBalanceResponse>,
    pub total_zec: f64,
}

/// Scan progress response
#[derive(Debug, Serialize)]
pub struct ScanProgressResponse {
    pub chain: String,
    pub scan_type: String,
    pub last_scanned_height: u64,
    pub chain_tip_height: u64,
    pub progress_percent: f64,
    pub estimated_seconds_remaining: Option<u64>,
    pub is_scanning: bool,
    pub notes_found: u64,
}

/// Fund source for transfers
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FundSource {
    /// Automatically use shielded first, then transparent
    Auto,
    /// Only use shielded (Orchard) balance
    Shielded,
    /// Only use transparent balance (shielding operation)
    Transparent,
}

impl Default for FundSource {
    fn default() -> Self {
        FundSource::Auto
    }
}

/// Orchard transfer request
#[derive(Debug, Deserialize)]
pub struct OrchardTransferRequest {
    pub wallet_id: i32,
    pub to_address: String,
    pub amount: String,
    pub amount_zatoshis: Option<u64>,
    pub memo: Option<String>,
    #[allow(dead_code)]
    pub target_pool: Option<String>,
    #[serde(default)]
    pub fund_source: FundSource,
}

/// Orchard transfer response
#[derive(Debug, Serialize)]
#[allow(dead_code)]
pub struct OrchardTransferResponse {
    pub transfer_id: i64,
    pub status: String,
    pub tx_hash: Option<String>,
}

/// Get all unified addresses for a wallet
pub async fn get_unified_addresses(
    wallet_service: web::Data<Arc<WalletService>>,
    path: web::Path<i32>,
) -> AppResult<HttpResponse> {
    let wallet_id = path.into_inner();

    let addresses = wallet_service.get_unified_addresses(wallet_id).await?;

    let response: Vec<UnifiedAddressInfo> = addresses
        .into_iter()
        .map(|addr| UnifiedAddressInfo {
            address: addr.address,
            has_orchard: addr.has_orchard,
            has_sapling: addr.has_sapling,
            has_transparent: addr.has_transparent,
            transparent_address: addr.transparent_address,
            address_index: addr.address_index,
            account_index: addr.account_index,
        })
        .collect();

    Ok(HttpResponse::Ok().json(response))
}

/// Enable Orchard for a Zcash wallet
pub async fn enable_orchard(
    wallet_service: web::Data<Arc<WalletService>>,
    user: AuthenticatedUser,
    path: web::Path<i32>,
    request: web::Json<EnableOrchardRequest>,
) -> AppResult<HttpResponse> {
    if user.role != "admin" {
        return Err(AppError::Forbidden("Only admin can enable Orchard".to_string()));
    }

    let wallet_id = path.into_inner();

    let (unified_address, viewing_key) = wallet_service
        .enable_orchard(wallet_id, request.birthday_height)
        .await?;

    let response = EnableOrchardResponse {
        unified_address: UnifiedAddressInfo {
            address: unified_address.address,
            has_orchard: unified_address.has_orchard,
            has_sapling: unified_address.has_sapling,
            has_transparent: unified_address.has_transparent,
            transparent_address: unified_address.transparent_address,
            address_index: unified_address.address_index,
            account_index: unified_address.account_index,
        },
        viewing_key,
    };

    Ok(HttpResponse::Ok().json(response))
}

/// Get shielded balance for a wallet
pub async fn get_shielded_balance(
    wallet_service: web::Data<Arc<WalletService>>,
    path: web::Path<i32>,
) -> AppResult<HttpResponse> {
    let wallet_id = path.into_inner();
    let balance = wallet_service.get_shielded_balance(wallet_id).await?;

    let response = ShieldedBalanceResponse {
        total_zatoshis: balance.total_zatoshis,
        spendable_zatoshis: balance.spendable_zatoshis,
        pending_zatoshis: balance.pending_zatoshis,
        note_count: balance.note_count,
        pool: balance.pool.map(|p| p.as_db_str().to_string()),
    };

    Ok(HttpResponse::Ok().json(response))
}

/// Note response for API
#[derive(Debug, Serialize)]
pub struct NoteResponse {
    pub id: i32,
    pub nullifier: String,
    pub value_zatoshis: u64,
    pub value_zec: f64,
    pub block_height: u64,
    pub tx_hash: String,
    pub is_spent: bool,
    pub memo: Option<String>,
}

/// Get unspent notes for a wallet
pub async fn get_unspent_notes(
    wallet_service: web::Data<Arc<WalletService>>,
    path: web::Path<i32>,
) -> AppResult<HttpResponse> {
    let wallet_id = path.into_inner();
    let notes = wallet_service.get_unspent_notes_from_db(wallet_id).await?;

    let response: Vec<NoteResponse> = notes
        .into_iter()
        .map(|n| NoteResponse {
            id: n.id,
            nullifier: n.nullifier,
            value_zatoshis: n.value_zatoshis,
            value_zec: n.value_zatoshis as f64 / 100_000_000.0,
            block_height: n.block_height,
            tx_hash: n.tx_hash,
            is_spent: n.is_spent,
            memo: n.memo,
        })
        .collect();

    Ok(HttpResponse::Ok().json(response))
}

/// Get combined balance (transparent + shielded)
pub async fn get_combined_balance(
    wallet_service: web::Data<Arc<WalletService>>,
    path: web::Path<i32>,
) -> AppResult<HttpResponse> {
    let wallet_id = path.into_inner();
    let balance = wallet_service.get_combined_zcash_balance(wallet_id).await?;

    let response = CombinedBalanceResponse {
        wallet_id: balance.wallet_id,
        address: balance.address,
        transparent_balance: balance.transparent_balance,
        shielded_balance: balance.shielded_balance.map(|b| ShieldedBalanceResponse {
            total_zatoshis: b.total_zatoshis,
            spendable_zatoshis: b.spendable_zatoshis,
            pending_zatoshis: b.pending_zatoshis,
            note_count: b.note_count,
            pool: b.pool.map(|p| p.as_db_str().to_string()),
        }),
        total_zec: balance.total_zec,
    };

    Ok(HttpResponse::Ok().json(response))
}

/// Get scan progress
pub async fn get_scan_progress(
    wallet_service: web::Data<Arc<WalletService>>,
) -> AppResult<HttpResponse> {
    let progress = wallet_service.get_scan_progress().await?;

    let response = ScanProgressResponse {
        chain: progress.chain,
        scan_type: progress.scan_type,
        last_scanned_height: progress.last_scanned_height,
        chain_tip_height: progress.chain_tip_height,
        progress_percent: progress.progress_percent,
        estimated_seconds_remaining: progress.estimated_seconds_remaining,
        is_scanning: progress.is_scanning,
        notes_found: progress.notes_found,
    };

    Ok(HttpResponse::Ok().json(response))
}

/// Trigger Orchard sync
pub async fn sync_orchard(
    wallet_service: web::Data<Arc<WalletService>>,
    user: AuthenticatedUser,
) -> AppResult<HttpResponse> {
    if user.role != "admin" {
        return Err(AppError::Forbidden("Only admin can trigger sync".to_string()));
    }

    let progress = wallet_service.sync_orchard().await?;

    let response = ScanProgressResponse {
        chain: progress.chain,
        scan_type: progress.scan_type,
        last_scanned_height: progress.last_scanned_height,
        chain_tip_height: progress.chain_tip_height,
        progress_percent: progress.progress_percent,
        estimated_seconds_remaining: progress.estimated_seconds_remaining,
        is_scanning: progress.is_scanning,
        notes_found: progress.notes_found,
    };

    Ok(HttpResponse::Ok().json(response))
}

/// Transfer proposal response
#[derive(Debug, Serialize)]
pub struct TransferProposalResponse {
    pub proposal_id: String,
    pub amount_zatoshis: u64,
    pub amount_zec: f64,
    pub fee_zatoshis: u64,
    pub fee_zec: f64,
    pub fund_source: String,
    pub is_shielding: bool,
    pub is_deshielding: bool,
    pub to_address: String,
    pub memo: Option<String>,
    pub expiry_height: u64,
}

/// Initiate an Orchard transfer
pub async fn initiate_orchard_transfer(
    wallet_service: web::Data<Arc<WalletService>>,
    user: AuthenticatedUser,
    request: web::Json<OrchardTransferRequest>,
) -> AppResult<HttpResponse> {
    if user.role != "admin" {
        return Err(AppError::Forbidden("Only admin can initiate transfers".to_string()));
    }

    // Convert fund source
    let fund_source = match request.fund_source {
        FundSource::Auto => crate::blockchain::zcash::orchard::transfer::FundSource::Auto,
        FundSource::Shielded => crate::blockchain::zcash::orchard::transfer::FundSource::Shielded,
        FundSource::Transparent => crate::blockchain::zcash::orchard::transfer::FundSource::Transparent,
    };

    // Create transfer proposal
    let proposal = wallet_service
        .create_privacy_transfer_proposal(
            request.wallet_id,
            &request.to_address,
            &request.amount,
            request.amount_zatoshis, // Pass zatoshis if provided by frontend
            request.memo.clone(),
            fund_source,
        )
        .await?;

    tracing::info!(
        "Orchard transfer proposal created: wallet={}, to={}, amount_zec={}, amount_zatoshis={}, fee={} zatoshis",
        request.wallet_id,
        request.to_address,
        request.amount,
        proposal.amount_zatoshis,
        proposal.fee_zatoshis
    );

    let response = TransferProposalResponse {
        proposal_id: proposal.proposal_id.clone(),
        amount_zatoshis: proposal.amount_zatoshis,
        amount_zec: proposal.amount_zatoshis as f64 / 100_000_000.0,
        fee_zatoshis: proposal.fee_zatoshis,
        fee_zec: proposal.fee_zatoshis as f64 / 100_000_000.0,
        fund_source: format!("{:?}", proposal.fund_source).to_lowercase(),
        is_shielding: proposal.is_shielding,
        is_deshielding: proposal.is_deshielding,
        to_address: proposal.to_address.clone(),
        memo: proposal.memo.clone(),
        expiry_height: proposal.expiry_height,
    };

    Ok(HttpResponse::Ok().json(response))
}

/// Execute transfer request
#[derive(Debug, Deserialize)]
pub struct ExecuteTransferRequest {
    pub wallet_id: i32,
    pub proposal_id: String,
    pub amount_zatoshis: u64,
    pub fee_zatoshis: u64,
    pub to_address: String,
    pub memo: Option<String>,
    pub fund_source: String,
    pub is_shielding: bool,
    #[serde(default)]
    pub is_deshielding: bool,
    pub expiry_height: u64,
}

/// Execute transfer response
#[derive(Debug, Serialize)]
pub struct ExecuteTransferResponse {
    pub tx_id: String,
    pub status: String,
    pub raw_tx: Option<String>,
    pub amount_zatoshis: u64,
    pub fee_zatoshis: u64,
}

/// Execute a pending Orchard transfer
pub async fn execute_orchard_transfer(
    wallet_service: web::Data<Arc<WalletService>>,
    user: AuthenticatedUser,
    path: web::Path<String>,
    request: Option<web::Json<ExecuteTransferRequest>>,
) -> AppResult<HttpResponse> {
    if user.role != "admin" {
        return Err(AppError::Forbidden("Only admin can execute transfers".to_string()));
    }

    let proposal_id = path.into_inner();

    // Get the request body
    let req = request.ok_or_else(|| {
        AppError::ValidationError("Request body required for execute".to_string())
    })?;

    // Validate proposal ID matches
    if req.proposal_id != proposal_id {
        return Err(AppError::ValidationError(
            "Proposal ID mismatch".to_string(),
        ));
    }

    tracing::info!(
        "Executing Orchard transfer: proposal={}, amount_zatoshis={}, fee_zatoshis={}, is_shielding={}, is_deshielding={}",
        proposal_id,
        req.amount_zatoshis,
        req.fee_zatoshis,
        req.is_shielding,
        req.is_deshielding
    );

    // CRITICAL SAFETY CHECK: Prevent zero-value transactions
    // A zero-value transaction would cause all input funds to become miner fees
    if req.amount_zatoshis == 0 {
        tracing::error!(
            "BLOCKED: Attempted to execute transfer with amount_zatoshis=0! proposal={}, to={}",
            proposal_id,
            req.to_address
        );
        return Err(AppError::ValidationError(
            "Transfer amount cannot be zero. This would result in complete fund loss.".to_string(),
        ));
    }

    // Additional sanity check: amount should be reasonable (at least 1000 zatoshis = 0.00001 ZEC)
    const MIN_TRANSFER_ZATOSHIS: u64 = 1000;
    if req.amount_zatoshis < MIN_TRANSFER_ZATOSHIS {
        tracing::warn!(
            "Transfer amount very small: {} zatoshis for proposal {}",
            req.amount_zatoshis,
            proposal_id
        );
    }

    // Safety check: fee should not exceed 0.001 ZEC (100,000 zatoshis)
    // ZIP-317 fees for shielding with change can be up to 15,000-20,000 zatoshis
    const MAX_FEE_ZATOSHIS: u64 = 100_000; // 0.001 ZEC - reasonable upper limit
    if req.fee_zatoshis > MAX_FEE_ZATOSHIS {
        tracing::error!(
            "BLOCKED: Fee ({} zatoshis) exceeds maximum allowed ({} zatoshis)! proposal={}",
            req.fee_zatoshis,
            MAX_FEE_ZATOSHIS,
            proposal_id
        );
        return Err(AppError::ValidationError(
            format!(
                "Fee ({} zatoshis = {} ZEC) exceeds maximum allowed (0.001 ZEC). This indicates a configuration error.",
                req.fee_zatoshis,
                req.fee_zatoshis as f64 / 100_000_000.0
            ),
        ));
    }

    // Warning if fee seems high but still acceptable
    if req.fee_zatoshis > 20_000 {
        tracing::warn!(
            "Fee is higher than typical: {} zatoshis ({} ZEC) for proposal {}",
            req.fee_zatoshis,
            req.fee_zatoshis as f64 / 100_000_000.0,
            proposal_id
        );
    }

    // Convert fund source
    let fund_source = match req.fund_source.as_str() {
        "shielded" => crate::blockchain::zcash::orchard::transfer::FundSource::Shielded,
        "transparent" => crate::blockchain::zcash::orchard::transfer::FundSource::Transparent,
        _ => crate::blockchain::zcash::orchard::transfer::FundSource::Auto,
    };

    // Reconstruct proposal
    let proposal = crate::blockchain::zcash::orchard::transfer::TransferProposal {
        proposal_id: req.proposal_id.clone(),
        amount_zatoshis: req.amount_zatoshis,
        fee_zatoshis: req.fee_zatoshis,
        fund_source,
        is_shielding: req.is_shielding,
        is_deshielding: req.is_deshielding,
        to_address: req.to_address.clone(),
        memo: req.memo.clone(),
        expiry_height: req.expiry_height,
        // Single-shot API transfer: change crosses to Ironwood post-NU6.3.
        // Staggered migration batches set their own policy (see ChangePolicy).
        change_policy: Default::default(),
    };

    // Execute the transfer
    let result = wallet_service
        .execute_privacy_transfer(req.wallet_id, &proposal)
        .await?;

    tracing::info!(
        "Orchard transfer executed: proposal={}, tx_id={}, status={:?}",
        proposal_id,
        result.tx_id,
        result.status
    );

    let response = ExecuteTransferResponse {
        tx_id: result.tx_id,
        status: format!("{:?}", result.status).to_lowercase(),
        raw_tx: result.raw_tx,
        amount_zatoshis: result.amount_zatoshis,
        fee_zatoshis: result.fee_zatoshis,
    };

    Ok(HttpResponse::Ok().json(response))
}
