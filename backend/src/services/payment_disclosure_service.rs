//! M1 F1.1 — PaymentDisclosure (ZIP-307) async generation framework.
//!
//! M1.W1 ship: the `generate` API records a disclosure row + spawns an
//! async task that does the heavy work and updates status to `ready`
//! (or `failed`).  The task body in M1.W1 produces a placeholder
//! disclosure JSON keyed by scope so the frontend can render the
//! report page end-to-end.  M1.W2 wires the real librustzcash payment
//! disclosure builder (depends on viewing key export being ready first).
//!
//! Status FSM:  generating  ->  ready  | failed
//! TTL: 7 days (NFR-4).  An out-of-scope cleaner job (M2) deletes expired
//! rows + files.

use std::sync::Arc;

use serde_json::json;

use crate::db::models_m1::{CreatePaymentDisclosureRequest, PaymentDisclosure};
use crate::db::repositories::PaymentDisclosureRepository;
use crate::error::{AppError, AppResult};

pub struct PaymentDisclosureService {
    repo: PaymentDisclosureRepository,
}

impl PaymentDisclosureService {
    pub fn new(repo: PaymentDisclosureRepository) -> Self {
        Self { repo }
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
        let granularity = req.granularity.clone();
        let format = req.format.clone();
        let scope_param = req.scope_param.clone();
        tokio::spawn(async move {
            let outcome = build_disclosure_body(&granularity, &format, &scope_param).await;
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

/// M1.W1 placeholder body.  Real librustzcash ZIP-307 payment_disclosure
/// build_payment_disclosure() integration is M1.W2; this returns a typed
/// JSON shaped like the eventual output so the frontend report page can
/// render the same fields end-to-end.
async fn build_disclosure_body(
    granularity: &str,
    format: &str,
    scope_param: &serde_json::Value,
) -> AppResult<(serde_json::Value, i32)> {
    // Simulate generation latency so the frontend can exercise the
    // polling state machine.  2s is short enough for dev test, long
    // enough that the UI definitely renders the generating state.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

    let now = chrono::Utc::now();
    let body = json!({
        "zip_version": "307-draft",
        "generated_at": now.to_rfc3339(),
        "granularity": granularity,
        "format": format,
        "scope": scope_param,
        // Placeholder until M1.W2: real bundles will go here, one per
        // disclosed Orchard action.
        "actions": [],
        "stub": true,
        "note": "M1.W1 framework: librustzcash payment_disclosure wired in W2"
    });
    Ok((body, 0))
}

// Re-export the Arc-friendly alias so handlers / main wire it uniformly.
pub type SharedPaymentDisclosureService = Arc<PaymentDisclosureService>;
