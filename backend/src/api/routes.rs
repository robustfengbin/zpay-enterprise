use actix_governor::{Governor, GovernorConfigBuilder};
use actix_web::web;
use std::sync::Arc;

use super::handlers;
use super::middleware::AuthMiddleware;
use crate::services::AuthService;

pub fn configure_routes(cfg: &mut web::ServiceConfig, auth_service: Arc<AuthService>) {
    // Throttle /auth/login to roughly 5 attempts per minute per peer IP.
    // The default key extractor (PeerIpKeyExtractor) buckets by client IP.
    // seconds_per_request(12) replenishes one slot every 12 s, burst_size(5)
    // allows a small initial burst — together this is ~5 requests/minute
    // steady-state per IP, which makes online password bruteforce
    // uneconomical without blocking real users who fat-finger a few times.
    // Bucket state is per process, so with 4 actix workers the effective
    // ceiling is ~20/min/IP — still well below what a credible bruteforcer
    // would need.
    let login_governor = GovernorConfigBuilder::default()
        .seconds_per_request(12)
        .burst_size(5)
        .finish()
        .expect("login governor config is statically valid");

    cfg.service(
        web::scope("/api/v1")
            // Public routes
            .service(
                web::resource("/auth/login")
                    .wrap(Governor::new(&login_governor))
                    .route(web::post().to(handlers::login)),
            )
            .route("/health", web::get().to(health_check))
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
                    .route("/transfers/orchard/{id}/execute", web::post().to(handlers::execute_orchard_transfer)),
            ),
    );
}

async fn health_check() -> actix_web::HttpResponse {
    actix_web::HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION")
    }))
}
