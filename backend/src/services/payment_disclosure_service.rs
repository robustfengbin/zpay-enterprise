//! M1 F1.1 — PaymentDisclosure (ZIP-307 inspired) generation service.
//!
//! M1.W2 ship: replaces the M1.W1 placeholder body with a real
//! enterprise-audit disclosure assembled from on-chain scanned notes.
//! The disclosure JSON follows the shape of a ZIP-307 payment disclosure
//! (per-action records keyed by tx_hash, with value / memo / nullifier
//! revealed) but skips the Halo 2 cryptographic proof component — that
//! requires sender-side spending-key cooperation and is M2 scope.  The
//! result is fully usable for the enterprise audit workflow because the
//! receiver already has the notes verifiably in their wallet, and the
//! revealed nullifier + block_height anchors each entry to the chain.
//!
//! Status FSM: generating -> ready | failed.
//! TTL: 7 days (NFR-4) — cleaner job deletes expired rows in M2.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::json;

use crate::blockchain::ChainRegistry;
use crate::db::models_m1::{CreatePaymentDisclosureRequest, PaymentDisclosure};
use crate::db::repositories::{
    OrchardRepository, PaymentDisclosureRepository, WalletRepository,
};
use crate::db::repositories::orchard_repo::StoredOrchardNote;
use crate::error::{AppError, AppResult};

pub struct PaymentDisclosureService {
    repo: PaymentDisclosureRepository,
    wallet_repo: WalletRepository,
    orchard_repo: OrchardRepository,
    chain_registry: Arc<ChainRegistry>,
}

impl PaymentDisclosureService {
    pub fn new(
        repo: PaymentDisclosureRepository,
        wallet_repo: WalletRepository,
        orchard_repo: OrchardRepository,
        chain_registry: Arc<ChainRegistry>,
    ) -> Self {
        Self {
            repo,
            wallet_repo,
            orchard_repo,
            chain_registry,
        }
    }

    pub async fn generate(
        &self,
        wallet_id: i32,
        user_id: i32,
        req: CreatePaymentDisclosureRequest,
    ) -> AppResult<PaymentDisclosure> {
        // Validate granularity + format up front (cheap, fail-fast).
        match req.granularity.as_str() {
            "tx" | "address" | "range" => {}
            other => {
                return Err(AppError::ValidationError(format!(
                    "invalid granularity '{}'; expected tx | address | range",
                    other
                )));
            }
        }
        match req.format.as_str() {
            "pdf" | "csv" | "json" => {}
            other => {
                return Err(AppError::ValidationError(format!(
                    "invalid format '{}'; expected pdf | csv | json",
                    other
                )));
            }
        }
        // Spot-check the scope_param payload matches the granularity claim;
        // catches the common frontend bug of sending "range" with only a
        // tx_hash and getting a misleading 200 + empty PDF later.
        let scope = &req.scope_param;
        match req.granularity.as_str() {
            "tx" if scope.get("tx_hash").is_none() => {
                return Err(AppError::ValidationError(
                    "scope_param must contain tx_hash for granularity=tx".to_string(),
                ));
            }
            "address" if scope.get("address").is_none() => {
                return Err(AppError::ValidationError(
                    "scope_param must contain address for granularity=address".to_string(),
                ));
            }
            "range"
                if scope.get("from").is_none() || scope.get("to").is_none() =>
            {
                return Err(AppError::ValidationError(
                    "scope_param must contain from + to for granularity=range".to_string(),
                ));
            }
            _ => {}
        }

        // Wallet must exist + be ZCash — disclosure pulls from Orchard notes.
        let wallet = self
            .wallet_repo
            .find_by_id(wallet_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("wallet {} not found", wallet_id)))?;
        if wallet.chain != "zcash" {
            return Err(AppError::ValidationError(format!(
                "payment disclosure only supported for zcash wallets (this is {})",
                wallet.chain
            )));
        }

        let id = self
            .repo
            .create(
                wallet_id,
                user_id,
                &req.granularity,
                &req.scope_param,
                &req.format,
            )
            .await?;

        // Spawn async task — caller returns immediately with status=generating.
        // We re-fetch in the spawned task so a fresh connection from the pool
        // handles the update rather than holding the request connection.
        let repo = self.repo.clone();
        let orchard_repo = self.orchard_repo.clone();
        let chain_registry = self.chain_registry.clone();
        let granularity = req.granularity.clone();
        let format = req.format.clone();
        let scope_param = req.scope_param.clone();
        let wallet_address = wallet.address.clone();
        let wallet_chain = wallet.chain.clone();
        tokio::spawn(async move {
            let outcome = build_disclosure_body(
                &orchard_repo,
                &chain_registry,
                wallet_id,
                &wallet_address,
                &wallet_chain,
                &granularity,
                &format,
                &scope_param,
            )
            .await;
            match outcome {
                Ok((body, tx_count)) => {
                    let _ = repo.mark_ready(id, &body, tx_count, None).await;
                }
                Err(e) => {
                    let _ = repo.mark_failed(id, &e.to_string()).await;
                }
            }
        });

        self.repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| AppError::InternalError("disclosure vanished after insert".to_string()))
    }

    pub async fn get(&self, id: i32) -> AppResult<PaymentDisclosure> {
        self.repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("payment_disclosure {} not found", id)))
    }

    pub async fn list_by_wallet(&self, wallet_id: i32) -> AppResult<Vec<PaymentDisclosure>> {
        self.repo.list_by_wallet(wallet_id).await
    }
}

/// Assemble the disclosure JSON from scanned Orchard notes filtered by
/// the requested granularity.  Returns (body, action_count).
///
/// The body shape is ZIP-307 inspired (per-action records keyed by
/// tx_hash with value / memo / nullifier revealed) but omits the
/// Halo 2 cryptographic proof — that requires sender cooperation and
/// is M2 scope.  For the enterprise audit workflow this is sufficient
/// because the receiver already has the notes verifiably in their
/// wallet and the revealed nullifier anchors each entry to the chain.
#[allow(clippy::too_many_arguments)]
async fn build_disclosure_body(
    orchard_repo: &OrchardRepository,
    chain_registry: &ChainRegistry,
    wallet_id: i32,
    wallet_address: &str,
    wallet_chain: &str,
    granularity: &str,
    format: &str,
    scope_param: &serde_json::Value,
) -> AppResult<(serde_json::Value, i32)> {
    // resolved_range carries (from_height, to_height, optional from_ts, optional to_ts)
    // so we can echo both representations in the body for the auditor.
    let mut resolved_range: Option<(u64, u64, Option<DateTime<Utc>>, Option<DateTime<Utc>>)> = None;

    let notes: Vec<StoredOrchardNote> = match granularity {
        "tx" => {
            let tx_hash = scope_param
                .get("tx_hash")
                .and_then(|v| v.as_str())
                .ok_or_else(|| {
                    AppError::ValidationError("scope_param.tx_hash missing".to_string())
                })?;
            orchard_repo
                .list_notes_by_tx_hash(wallet_id, tx_hash)
                .await?
        }
        "address" => {
            // M1: a wallet has a single primary address; the auditor's
            // granularity=address request is interpreted as "everything
            // the wallet received" which maps to all scanned notes.  M2
            // can refine when multi-address-per-wallet ships.
            orchard_repo.list_all_notes_by_wallet(wallet_id).await?
        }
        "range" => {
            let chain_client = chain_registry.get(wallet_chain)?;
            let (from_h, from_ts) =
                resolve_range_endpoint(chain_client.as_ref(), scope_param, "from").await?;
            let (to_h, to_ts) =
                resolve_range_endpoint(chain_client.as_ref(), scope_param, "to").await?;
            if from_h > to_h {
                return Err(AppError::ValidationError(format!(
                    "resolved from_height ({}) must be <= to_height ({})",
                    from_h, to_h
                )));
            }
            resolved_range = Some((from_h, to_h, from_ts, to_ts));
            orchard_repo
                .list_notes_in_height_range(wallet_id, from_h, to_h)
                .await?
        }
        other => {
            return Err(AppError::ValidationError(format!(
                "unsupported granularity '{}'",
                other
            )));
        }
    };

    let now = chrono::Utc::now();
    let actions: Vec<serde_json::Value> = notes
        .iter()
        .map(|n| {
            json!({
                "tx_hash": n.tx_hash,
                "block_height": n.block_height,
                "position_in_block": n.position_in_block,
                "value_zatoshis": n.value_zatoshis,
                // 1 ZEC = 1e8 zatoshis; we emit both so PDF/CSV layers
                // do not have to redo the conversion.
                "value_zec": (n.value_zatoshis as f64) / 100_000_000.0,
                "memo": n.memo,
                "nullifier": n.nullifier,
                "is_spent": n.is_spent,
                "spent_in_tx": n.spent_in_tx,
                "recipient_address_hex": n.recipient,
            })
        })
        .collect();
    let tx_count = actions.len() as i32;

    let resolved_range_json = resolved_range.map(|(fh, th, fts, tts)| {
        json!({
            "from_height": fh,
            "to_height": th,
            "from_ts": fts.map(|t| t.to_rfc3339()),
            "to_ts": tts.map(|t| t.to_rfc3339()),
        })
    });

    let body = json!({
        "zip_version": "307-enterprise",
        "generated_at": now.to_rfc3339(),
        "wallet_address": wallet_address,
        "granularity": granularity,
        "format": format,
        "scope": scope_param,
        "resolved_range": resolved_range_json,
        "actions": actions,
        "action_count": tx_count,
        "notes": "ZIP-307 inspired enterprise audit body. Records derive from \
                  the wallet's scanned Orchard notes (receiver-side IVK \
                  decryption already performed during sync). Halo 2 \
                  cryptographic proof component is M2 scope and requires \
                  sender-side spending-key cooperation."
    });
    Ok((body, tx_count))
}

/// Parse one endpoint of a `range` scope.  Accepts:
///   * u64 — interpreted directly as a block height
///   * ISO 8601 string — converted to a block height via the chain's
///     `block_at_timestamp` method, with the parsed DateTime echoed back
///     so the body can show both representations.
async fn resolve_range_endpoint(
    chain_client: &dyn crate::blockchain::ChainClient,
    scope_param: &serde_json::Value,
    key: &str,
) -> AppResult<(u64, Option<DateTime<Utc>>)> {
    let value = scope_param
        .get(key)
        .ok_or_else(|| AppError::ValidationError(format!("scope_param.{} missing", key)))?;

    if let Some(n) = value.as_u64() {
        return Ok((n, None));
    }
    if let Some(s) = value.as_str() {
        let parsed = DateTime::parse_from_rfc3339(s).map_err(|e| {
            AppError::ValidationError(format!(
                "scope_param.{} must be a u64 height or ISO 8601 string: {}",
                key, e
            ))
        })?;
        let ts_utc = parsed.with_timezone(&Utc);
        let height = chain_client.block_at_timestamp(ts_utc.timestamp()).await?;
        return Ok((height, Some(ts_utc)));
    }
    Err(AppError::ValidationError(format!(
        "scope_param.{} must be a u64 height or ISO 8601 string",
        key
    )))
}

// Re-export the Arc-friendly alias so handlers / main wire it uniformly.
pub type SharedPaymentDisclosureService = Arc<PaymentDisclosureService>;
