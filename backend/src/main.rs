mod api;
mod blockchain;
mod config;
mod crypto;
mod db;
mod error;
mod services;

use actix_cors::Cors;
use actix_web::{web, App, HttpServer};
use std::sync::Arc;
use tokio::time::{interval, Duration};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use api::handlers::load_rpc_config_from_db;
use blockchain::{ethereum::EthereumClient, zcash::ZcashClient, ChainRegistry};
use config::AppConfig;
use db::repositories::{SettingsRepository, TransferRepository, UserRepository, WalletRepository};
use services::{AuthService, TransferService, WalletService};

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialize logging with console and file output
    let log_dir = std::env::var("LOG_DIR").unwrap_or_else(|_| "logs".into());
    std::fs::create_dir_all(&log_dir).expect("Failed to create log directory");

    // File appender - rotates when > 500MB, keeps 10 backup files
    let log_path = std::path::Path::new(&log_dir).join("web3-wallet.log");
    let file_appender = rolling_file::RollingFileAppender::new(
        log_path,
        rolling_file::RollingConditionBasic::new()
            .max_size(500 * 1024 * 1024), // 500MB
        10, // Keep 10 backup files
    )
    .expect("Failed to create log file appender");

    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = tracing_subscriber::EnvFilter::new(
        std::env::var("RUST_LOG").unwrap_or_else(|_| "info,sqlx=warn".into()),
    );

    tracing_subscriber::registry()
        .with(env_filter)
        // Console output
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(false)
        )
        // File output
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_ansi(false)
                .with_writer(non_blocking)
        )
        .init();

    // Keep the guard alive for the duration of the program
    let _log_guard = _guard;

    tracing::info!("Starting Web3 Wallet Service");

    // Load configuration
    let config = AppConfig::load().expect("Failed to load configuration");
    tracing::info!("Configuration loaded successfully");
    tracing::info!("Database: {}:{}/{}", config.database.host, config.database.port, config.database.name);

    // Create database connection pool
    let pool = db::create_pool(&config.database)
        .await
        .expect("Failed to create database pool");

    // Run migrations
    db::run_migrations(&pool)
        .await
        .expect("Failed to run database migrations");

    // Initialize repositories
    let user_repo = UserRepository::new(pool.clone());
    let wallet_repo = WalletRepository::new(pool.clone());
    let transfer_repo = TransferRepository::new(pool.clone());
    let settings_repo = Arc::new(SettingsRepository::new(pool.clone()));

    // Load RPC configuration from database (or use defaults from .env)
    let rpc_config = load_rpc_config_from_db(
        &settings_repo,
        &config.ethereum.rpc_url,
        &config.ethereum.fallback_rpcs,
    )
    .await;

    // Create a modified ethereum config with the loaded RPC settings
    let mut eth_config = config.ethereum.clone();
    eth_config.rpc_url = rpc_config.primary_rpc;
    eth_config.fallback_rpcs = rpc_config.fallback_rpcs;

    // Initialize Ethereum client with loaded config
    let eth_client = Arc::new(
        EthereumClient::new(&eth_config).expect("Failed to create Ethereum client"),
    );

    // Initialize Zcash client
    let zcash_client = Arc::new(
        ZcashClient::new(&config.zcash).expect("Failed to create Zcash client"),
    );

    // Initialize chain registry
    let mut chain_registry = ChainRegistry::new();
    chain_registry.register(eth_client.clone());
    chain_registry.register(zcash_client.clone());

    let chain_registry = Arc::new(chain_registry);

    // Initialize services
    let auth_service = Arc::new(AuthService::new(user_repo, config.jwt.clone()));
    let wallet_service = Arc::new(WalletService::new(
        wallet_repo,
        chain_registry.clone(),
        config.security.clone(),
        pool.clone(),
    ));
    let transfer_service = Arc::new(TransferService::new(
        transfer_repo,
        wallet_service.clone(),
        chain_registry.clone(),
    ));

    // Create default admin user using the password supplied via
    // WEB3_SECURITY__ADMIN_INITIAL_PASSWORD (validated in config::validate).
    auth_service
        .create_default_admin(&config.security.admin_initial_password)
        .await
        .expect("Failed to create default admin");

    // Start background task for checking pending transfers
    let transfer_service_bg = transfer_service.clone();
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            if let Err(e) = transfer_service_bg.check_pending_transfers().await {
                tracing::error!("Error checking pending transfers: {}", e);
            }
        }
    });

    // Pre-build Orchard proving key in background (expensive one-time operation)
    // This ensures the first privacy transfer doesn't have to wait
    tokio::spawn(async move {
        tracing::info!("Pre-building Orchard proving key in background...");
        // Run in blocking task since it's CPU-intensive
        let result = tokio::task::spawn_blocking(|| {
            blockchain::zcash::orchard::init_proving_key();
        }).await;
        match result {
            Ok(_) => tracing::info!("Orchard proving key ready"),
            Err(e) => tracing::warn!("Failed to pre-build Orchard proving key: {}", e),
        }
    });

    // Start Orchard background sync task (syncs all Zcash wallets every 5 minutes)
    wallet_service.clone().start_background_sync();

    let server_host = config.server.host.clone();
    let server_port = config.server.port;
    let auth_service_for_routes = auth_service.clone();

    tracing::info!("Starting HTTP server at {}:{}", server_host, server_port);

    let settings_repo_for_app = settings_repo.clone();
    let eth_client_for_app = eth_client.clone();
    let allowed_origin = config.server.allowed_origin.clone();

    HttpServer::new(move || {
        // CORS: explicit allowlist of one origin. NEVER use allow_any_origin()
        // here — this service holds wallet keys, so accepting credentialed
        // cross-origin requests from arbitrary websites is a wallet-drain
        // vector. See SECURITY.md and the 2026-04-25 audit for rationale.
        let cors = Cors::default()
            .allowed_origin(&allowed_origin)
            .allowed_methods(vec!["GET", "POST", "PUT", "DELETE", "OPTIONS"])
            .allowed_headers(vec![
                actix_web::http::header::AUTHORIZATION,
                actix_web::http::header::CONTENT_TYPE,
            ])
            .supports_credentials()
            .max_age(3600);

        // Security response headers. The backend itself only serves JSON, so
        // CSP `default-src 'none'` plus `frame-ancestors 'none'` is the
        // strictest correct value — nothing should be loaded *from* an API
        // response. X-Frame-Options + X-Content-Type-Options + Referrer-Policy
        // are belt-and-braces against legacy browser behavior. The frontend
        // (served from a separate origin / static host) sets its own CSP.
        let security_headers = actix_web::middleware::DefaultHeaders::new()
            .add((
                "Content-Security-Policy",
                "default-src 'none'; frame-ancestors 'none'",
            ))
            .add(("X-Frame-Options", "DENY"))
            .add(("X-Content-Type-Options", "nosniff"))
            .add(("Referrer-Policy", "strict-origin-when-cross-origin"));

        App::new()
            .wrap(cors)
            .wrap(security_headers)
            .wrap(actix_web::middleware::from_fn(api::middleware::request_logger))
            // Bound request bodies to prevent OOM-by-payload DoS. Most
            // handlers move JSON < 8 KB; 64 KB JSON / 256 KB raw covers
            // wallet imports (which carry encrypted keys) with headroom.
            .app_data(web::JsonConfig::default().limit(64 * 1024))
            .app_data(web::PayloadConfig::new(256 * 1024))
            .app_data(web::Data::new(auth_service.clone()))
            .app_data(web::Data::new(wallet_service.clone()))
            .app_data(web::Data::new(transfer_service.clone()))
            .app_data(web::Data::new(chain_registry.clone()))
            .app_data(web::Data::new(settings_repo_for_app.clone()))
            .app_data(web::Data::new(eth_client_for_app.clone()))
            .configure(|cfg| api::configure_routes(cfg, auth_service_for_routes.clone()))
    })
    .bind((server_host, server_port))?
    .run()
    .await
}
