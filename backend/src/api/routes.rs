use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::web;
use std::sync::Arc;

use super::handlers;
use super::middleware::{AuditorAuthMiddleware, AuthMiddleware};
use crate::services::{AuditorService, AuthService};

pub fn configure_routes(
    cfg: &mut web::ServiceConfig,
    auth_service: Arc<AuthService>,
    auditor_service: Arc<AuditorService>,
) {
    // Throttle /auth/login to roughly 5 attempts per minute per peer IP.
    // The default key extractor (PeerIpKeyExtractor) buckets by client IP.
    // seconds_per_request(12) replenishes one slot every 12 s, burst_size(5)
    // allows a small initial burst — together this is ~5 requests/minute
    // steady-state per IP, which makes online password bruteforce
    // uneconomical without blocking real users who fat-finger a few times.
    //
    // The Governor is applied to a dedicated `/auth/login` scope rather
    // than to the route directly because actix-web 4's per-route `.wrap()`
    // path doesn't pick up actix-governor's middleware in some
    // configurations — scoping the limiter is the documented pattern.
    let login_governor = GovernorConfigBuilder::default()
        .seconds_per_request(12)
        .burst_size(5)
        .finish()
        .expect("login governor config is statically valid");

    cfg.service(
        web::scope("/api/v1")
            // Public routes
            .service(
                web::scope("/auth/login")
                    .wrap(Governor::new(&login_governor))
                    .route("", web::post().to(handlers::login)),
            )
            .route("/health", web::get().to(health_check))
            // ============================================================
            // M1 — F1.1 Auditor side (independent prefix).
            // /auditor/login is public so an auditor can obtain a token.
            // Every other /auditor/* route is wrapped in AuditorAuthMiddleware
            // which verifies kind=auditor JWTs (separate from the user-side
            // AuthMiddleware below).  Scoping protects the catch-all
            // `web::scope("")` from accidentally swallowing these routes.
            // ============================================================
            .route("/auditor/login", web::post().to(handlers::m1::auditor_login))
            .service(
                web::scope("/auditor")
                    .wrap(AuditorAuthMiddleware {
                        auditor_service: auditor_service.clone(),
                    })
                    .route("/me", web::get().to(handlers::m1::auditor_me))
                    .route("/wallets", web::get().to(handlers::m1::auditor_list_wallets))
                    .route(
                        "/wallets/{id}/balance",
                        web::get().to(handlers::m1::auditor_wallet_balance),
                    )
                    .route(
                        "/wallets/{id}/transfers",
                        web::get().to(handlers::m1::auditor_wallet_transfers),
                    )
                    .route(
                        "/wallets/{id}/disclosures",
                        web::get().to(handlers::m1::auditor_wallet_disclosures),
                    ),
            )
            // Protected routes
            .service(
                web::scope("")
                    .wrap(AuthMiddleware { auth_service })
                    // Auth routes
                    .route("/auth/logout", web::post().to(handlers::logout))
                    .route("/auth/password", web::put().to(handlers::change_password))
                    .route("/auth/me", web::get().to(handlers::me))
                    // Wallet routes
                    .route("/wallets", web::get().to(handlers::list_wallets))
                    .route("/wallets", web::post().to(handlers::create_wallet))
                    .route("/wallets/import", web::post().to(handlers::import_wallet))
                    .route("/wallets/balance", web::get().to(handlers::get_balance))
                    .route("/wallets/{id}", web::get().to(handlers::get_wallet))
                    .route("/wallets/{id}", web::delete().to(handlers::delete_wallet))
                    .route("/wallets/{id}/activate", web::put().to(handlers::set_active_wallet))
                    .route("/wallets/{id}/export-key", web::post().to(handlers::export_private_key))
                    // Transfer routes
                    .route("/transfers", web::get().to(handlers::list_transfers))
                    .route("/transfers", web::post().to(handlers::initiate_transfer))
                    .route("/transfers/estimate-gas", web::post().to(handlers::estimate_gas))
                    .route("/transfers/{id}", web::get().to(handlers::get_transfer))
                    .route("/transfers/{id}/execute", web::post().to(handlers::execute_transfer))
                    // Chain routes
                    .route("/chains", web::get().to(handlers::list_chains))
                    // Settings routes
                    .route("/settings/rpc/presets", web::get().to(handlers::get_rpc_presets))
                    .route("/settings/rpc", web::get().to(handlers::get_rpc_config))
                    .route("/settings/rpc", web::put().to(handlers::update_rpc_config))
                    .route("/settings/rpc/test", web::post().to(handlers::test_rpc_endpoint))
                    // Orchard (Zcash privacy) routes
                    .route("/wallets/{id}/orchard/enable", web::post().to(handlers::enable_orchard))
                    .route("/wallets/{id}/orchard/addresses", web::get().to(handlers::get_unified_addresses))
                    .route("/wallets/{id}/orchard/balance", web::get().to(handlers::get_shielded_balance))
                    .route("/wallets/{id}/orchard/balance/combined", web::get().to(handlers::get_combined_balance))
                    .route("/wallets/{id}/orchard/notes", web::get().to(handlers::get_unspent_notes))
                    .route("/zcash/scan/status", web::get().to(handlers::get_scan_progress))
                    .route("/zcash/scan/sync", web::post().to(handlers::sync_orchard))
                    .route("/transfers/orchard", web::post().to(handlers::initiate_orchard_transfer))
                    .route("/transfers/orchard/{id}/execute", web::post().to(handlers::execute_orchard_transfer))
                    // ============================================================
                    // M1 2026-06 — F1.1 Viewing Key audit (Admin side)
                    // ============================================================
                    .route("/wallets/{id}/viewing-keys/export", web::post().to(handlers::m1::export_viewing_key))
                    .route("/wallets/{id}/viewing-keys/exports", web::get().to(handlers::m1::list_viewing_key_exports))
                    .route("/viewing-keys/download/{token}", web::get().to(handlers::m1::download_viewing_key))
                    .route("/auditors", web::post().to(handlers::m1::create_auditor))
                    .route("/auditors", web::get().to(handlers::m1::list_auditors))
                    // POST + PUT both accepted — different frontend conventions
                    // (some clients only POST mutation endpoints).
                    .route("/auditors/{id}/deactivate", web::put().to(handlers::m1::deactivate_auditor))
                    .route("/auditors/{id}/deactivate", web::post().to(handlers::m1::deactivate_auditor))
                    .route("/wallets/{id}/payment-disclosures", web::post().to(handlers::m1::create_payment_disclosure))
                    .route("/wallets/{id}/payment-disclosures", web::get().to(handlers::m1::list_payment_disclosures))
                    .route("/payment-disclosures/{id}", web::get().to(handlers::m1::get_payment_disclosure))
                    .route("/payment-disclosures/{id}/download", web::get().to(handlers::m1::download_payment_disclosure))
                    // ============================================================
                    // M1 — F2.1 Maker / Checker dual-sign
                    // ============================================================
                    .route("/transfers/{id}/approve", web::post().to(handlers::m1::approve_transfer))
                    .route("/transfers/{id}/reject", web::post().to(handlers::m1::reject_transfer))
                    .route("/transfers/{id}/approvals", web::get().to(handlers::m1::list_approvals_for_transfer))
                    .route("/approvals/pending", web::get().to(handlers::m1::list_pending_approvals))
                    .route("/approval-policies", web::get().to(handlers::m1::list_approval_policies))
                    .route("/approval-policies", web::post().to(handlers::m1::create_approval_policy))
                    .route("/approval-policies/{id}", web::put().to(handlers::m1::update_approval_policy))
                    .route("/approval-policies/{id}", web::delete().to(handlers::m1::delete_approval_policy))
                    // ============================================================
                    // M1 — F3.1 batch Payroll Run
                    // ============================================================
                    .route("/payroll/employees", web::post().to(handlers::m1::create_employee))
                    .route("/payroll/employees", web::get().to(handlers::m1::list_employees))
                    .route("/payroll/employees/{id}", web::get().to(handlers::m1::get_employee))
                    .route("/payroll/employees/{id}", web::put().to(handlers::m1::update_employee))
                    .route("/payroll/employees/{id}", web::delete().to(handlers::m1::delete_employee))
                    .route("/payroll/runs", web::post().to(handlers::m1::create_payroll_run))
                    .route("/payroll/runs", web::get().to(handlers::m1::list_payroll_runs))
                    .route("/payroll/runs/{id}", web::get().to(handlers::m1::get_payroll_run))
                    .route("/payroll/runs/{id}/execute", web::post().to(handlers::m1::execute_payroll_run))
                    .route("/payroll/runs/{id}/cancel", web::post().to(handlers::m1::cancel_payroll_run))
                    .route("/payroll/runs/{run_id}/items/{item_id}/retry", web::post().to(handlers::m1::retry_payroll_item))
                    .route("/payroll/runs/{id}/report", web::get().to(handlers::m1::payroll_run_report))
                    // ============================================================
                    // F4.1 2026-07 — Orchard → Ironwood migration runs
                    // ============================================================
                    .route("/migrations", web::post().to(handlers::migration::create_migration_run))
                    .route("/migrations", web::get().to(handlers::migration::list_migration_runs))
                    .route("/migrations/{id}", web::get().to(handlers::migration::get_migration_run))
                    .route("/migrations/{id}/execute", web::post().to(handlers::migration::execute_migration_run))
                    .route("/migrations/{id}/approve", web::post().to(handlers::migration::approve_migration_run))
                    .route("/migrations/{id}/reject", web::post().to(handlers::migration::reject_migration_run))
                    .route("/migrations/{id}/cancel", web::post().to(handlers::migration::cancel_migration_run))
                    .route("/migrations/{run_id}/items/{item_id}/retry", web::post().to(handlers::migration::retry_migration_item))
                    .route("/wallets/{id}/migration-status", web::get().to(handlers::migration::wallet_migration_status)),
            ),
    );
}

async fn health_check() -> actix_web::HttpResponse {
    actix_web::HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION")
    }))
}
