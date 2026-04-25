use config::{Config, ConfigError, Environment, File};
use rand::{distributions::Alphanumeric, Rng};
use serde::Deserialize;
use std::fs;
use std::io::Write;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    /// Allowed CORS origin for browser clients (the frontend URL). Required;
    /// wildcard `*` is explicitly rejected because this service holds wallet
    /// keys and must not accept credentialed requests from arbitrary origins.
    /// Set via WEB3_SERVER__ALLOWED_ORIGIN.
    pub allowed_origin: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub name: String,
    pub max_connections: u32,
}

impl DatabaseConfig {
    pub fn url(&self) -> String {
        format!(
            "mysql://{}:{}@{}:{}/{}",
            self.user, self.password, self.host, self.port, self.name
        )
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct JwtConfig {
    pub secret: String,
    pub expire_hours: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecurityConfig {
    pub encryption_key: String,
    /// Initial password for the default admin account. Required on first startup.
    /// Must be set via WEB3_SECURITY__ADMIN_INITIAL_PASSWORD env var or config file.
    pub admin_initial_password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EthereumConfig {
    pub rpc_url: String,
    pub chain_id: u64,
    pub fallback_rpcs: Vec<String>,
    /// HTTP/HTTPS/SOCKS5 proxy for RPC requests (e.g., "http://127.0.0.1:7890" or "socks5://127.0.0.1:1080")
    pub rpc_proxy: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ZcashConfig {
    pub rpc_url: String,
    pub fallback_rpcs: Vec<String>,
    /// HTTP/HTTPS/SOCKS5 proxy for RPC requests
    pub rpc_proxy: Option<String>,
    /// RPC username for authentication
    pub rpc_user: Option<String>,
    /// RPC password for authentication
    pub rpc_password: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub jwt: JwtConfig,
    pub security: SecurityConfig,
    pub ethereum: EthereumConfig,
    pub zcash: ZcashConfig,
}

impl AppConfig {
    pub fn load() -> Result<Self, ConfigError> {
        // Load .env file if exists
        let _ = dotenvy::dotenv();

        // Auto-generate any required secrets that are missing. Persists to
        // backend/.env.secrets so restarts reuse the same values — regenerating
        // would make existing encrypted wallets unrecoverable.
        ensure_secrets();

        // Re-load .env.secrets in case we just created or appended to it.
        let _ = dotenvy::from_path(".env.secrets");

        let config = Config::builder()
            // Server defaults
            .set_default("server.host", "127.0.0.1")?
            .set_default("server.port", 8080)?
            // Default CORS origin matches the dev frontend (Vite default port).
            // Production deployments MUST override via WEB3_SERVER__ALLOWED_ORIGIN.
            .set_default("server.allowed_origin", "http://localhost:3000")?
            // Database defaults
            .set_default("database.host", "localhost")?
            .set_default("database.port", 3306)?
            .set_default("database.user", "root")?
            .set_default("database.password", "")?
            .set_default("database.name", "web3_wallet")?
            .set_default("database.max_connections", 20)?
            // JWT defaults
            .set_default("jwt.secret", "change-me-in-production-please!")?
            .set_default("jwt.expire_hours", 24)?
            // Security defaults
            .set_default("security.encryption_key", "32-byte-encryption-key-here!!!!!")?
            // No default for admin_initial_password — required to be explicitly set
            .set_default("security.admin_initial_password", "")?
            // Ethereum defaults
            .set_default("ethereum.chain_id", 1)?
            .set_default("ethereum.rpc_url", "https://eth.llamarpc.com")?
            .set_default(
                "ethereum.fallback_rpcs",
                vec![
                    "https://rpc.ankr.com/eth",
                    "https://ethereum.publicnode.com",
                    "https://1rpc.io/eth",
                ],
            )?
            // RPC proxy (optional) - can be set via WEB3_ETHEREUM__RPC_PROXY env var
            .set_default("ethereum.rpc_proxy", Option::<String>::None)?
            // Zcash defaults
            .set_default("zcash.rpc_url", "http://127.0.0.1:8232")?
            .set_default("zcash.fallback_rpcs", Vec::<String>::new())?
            .set_default("zcash.rpc_proxy", Option::<String>::None)?
            .set_default("zcash.rpc_user", Option::<String>::None)?
            .set_default("zcash.rpc_password", Option::<String>::None)?
            // Load from config.toml if exists
            .add_source(File::with_name("config").required(false))
            // Override with environment variables (prefix: WEB3_)
            // Use __ as separator so WEB3_SECURITY__ENCRYPTION_KEY -> security.encryption_key
            .add_source(
                Environment::with_prefix("WEB3")
                    .prefix_separator("_")
                    .separator("__")
                    .try_parsing(true),
            )
            .build()?;

        let app_config: AppConfig = config.try_deserialize()?;

        // Validate configuration
        app_config.validate()?;

        Ok(app_config)
    }

    fn validate(&self) -> Result<(), ConfigError> {
        // Validate encryption key length (must be 32 bytes for AES-256)
        if self.security.encryption_key.len() != 32 {
            return Err(ConfigError::Message(
                "Encryption key must be exactly 32 bytes".to_string(),
            ));
        }

        // Validate CORS allowed_origin: must be set, must not be "*"
        let origin = self.server.allowed_origin.trim();
        if origin.is_empty() {
            return Err(ConfigError::Message(
                "WEB3_SERVER__ALLOWED_ORIGIN must be set to the frontend URL \
                 (e.g. https://app.example.com). Wildcard `*` is not allowed \
                 because this service holds wallet keys."
                    .to_string(),
            ));
        }
        if origin == "*" || origin.contains(',') {
            return Err(ConfigError::Message(
                "WEB3_SERVER__ALLOWED_ORIGIN must be a single origin (no wildcard, \
                 no comma-separated list). For multi-origin deployments, run \
                 separate backend instances behind a reverse proxy."
                    .to_string(),
            ));
        }
        if !(origin.starts_with("http://") || origin.starts_with("https://")) {
            return Err(ConfigError::Message(
                "WEB3_SERVER__ALLOWED_ORIGIN must begin with http:// or https:// \
                 (e.g. https://app.example.com)."
                    .to_string(),
            ));
        }

        // Validate admin initial password is set and not a weak default
        let pw = &self.security.admin_initial_password;
        if pw.is_empty() {
            return Err(ConfigError::Message(
                "WEB3_SECURITY__ADMIN_INITIAL_PASSWORD must be set before first startup. \
                 See backend/.env.example."
                    .to_string(),
            ));
        }
        if pw == "admin123" || pw.len() < 12 {
            return Err(ConfigError::Message(
                "WEB3_SECURITY__ADMIN_INITIAL_PASSWORD is too weak \
                 (must be at least 12 characters and not 'admin123')."
                    .to_string(),
            ));
        }

        // Validate JWT secret is not empty
        if self.jwt.secret.is_empty() {
            return Err(ConfigError::Message(
                "JWT secret cannot be empty".to_string(),
            ));
        }

        // Validate database config
        if self.database.host.is_empty() {
            return Err(ConfigError::Message(
                "Database host cannot be empty".to_string(),
            ));
        }
        if self.database.name.is_empty() {
            return Err(ConfigError::Message(
                "Database name cannot be empty".to_string(),
            ));
        }

        Ok(())
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 8080,
                allowed_origin: "http://localhost:3000".to_string(),
            },
            database: DatabaseConfig {
                host: "localhost".to_string(),
                port: 3306,
                user: "root".to_string(),
                password: "password".to_string(),
                name: "web3_wallet".to_string(),
                max_connections: 20,
            },
            jwt: JwtConfig {
                secret: "change-me-in-production".to_string(),
                expire_hours: 24,
            },
            security: SecurityConfig {
                encryption_key: "32-byte-encryption-key-here!!!!!".to_string(),
                admin_initial_password: String::new(),
            },
            ethereum: EthereumConfig {
                rpc_url: "https://eth.llamarpc.com".to_string(),
                chain_id: 1,
                fallback_rpcs: vec![
                    "https://rpc.ankr.com/eth".to_string(),
                    "https://ethereum.publicnode.com".to_string(),
                    "https://1rpc.io/eth".to_string(),
                ],
                rpc_proxy: None,
            },
            zcash: ZcashConfig {
                rpc_url: "http://127.0.0.1:8232".to_string(),
                fallback_rpcs: vec![],
                rpc_proxy: None,
                rpc_user: None,
                rpc_password: None,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Auto-generated secrets
// ---------------------------------------------------------------------------

/// Secrets that will be auto-generated (random, ASCII-alphanumeric) if they
/// are missing from the process environment at startup time. Each tuple is
/// (environment variable name, generated length in characters).
const AUTO_GENERATED_SECRETS: &[(&str, usize)] = &[
    // AES-256-GCM key must be exactly 32 bytes. Alphanumeric chars are ASCII
    // single-byte, so a 32-char string == 32 bytes.
    ("WEB3_SECURITY__ENCRYPTION_KEY", 32),
    // JWT signing secret. 64 chars is overkill-safe for HS256.
    ("WEB3_JWT__SECRET", 64),
    // Initial admin password. 24 chars easily clears the >=12-char validator
    // and is not "admin123".
    ("WEB3_SECURITY__ADMIN_INITIAL_PASSWORD", 24),
];

/// Relative path to the auto-generated secrets file. Kept next to `.env` for
/// operator familiarity. MUST remain gitignored.
const SECRETS_FILE: &str = ".env.secrets";

/// Fills in any missing critical secrets by generating random values and
/// persisting them to `backend/.env.secrets`. Called once at config load.
///
/// Behavior:
///
/// * If the environment variable is already set (non-empty), it is left alone.
/// * If `.env.secrets` already contains an entry, that value is loaded into
///   the process environment.
/// * Otherwise, a fresh random value is generated, set in the current process
///   environment, and appended to `.env.secrets`.
///
/// The file is created with `0600` permissions on Unix. On any I/O error we
/// log a warning and continue — config validation downstream will surface a
/// clearer error if the missing secret is truly unset.
///
/// Rationale: new operators should not be blocked by "how do I produce a
/// 32-byte encryption key" questions. At the same time, we never silently
/// *rotate* existing secrets (that would make encrypted wallets unrecoverable),
/// so `.env.secrets` values take priority over newly-generated ones.
fn ensure_secrets() {
    let path = Path::new(SECRETS_FILE);

    // Parse any existing .env.secrets entries so we can reuse (not rotate)
    // previously-generated values on restart.
    let existing: std::collections::HashMap<String, String> = fs::read_to_string(path)
        .ok()
        .map(|content| {
            content
                .lines()
                .filter_map(|line| {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') {
                        return None;
                    }
                    let (k, v) = line.split_once('=')?;
                    Some((k.trim().to_string(), v.trim().to_string()))
                })
                .collect()
        })
        .unwrap_or_default();

    let mut newly_generated: Vec<(&'static str, String)> = Vec::new();

    for (name, length) in AUTO_GENERATED_SECRETS {
        // Already set in env (from .env or the container runtime)? Leave it.
        if std::env::var(name).map(|v| !v.is_empty()).unwrap_or(false) {
            continue;
        }

        // Present in .env.secrets from a previous run? Load it.
        if let Some(value) = existing.get(*name) {
            if !value.is_empty() {
                std::env::set_var(name, value);
                continue;
            }
        }

        // Otherwise, generate a fresh one.
        let value: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(*length)
            .map(char::from)
            .collect();
        std::env::set_var(name, &value);
        newly_generated.push((name, value));
    }

    if newly_generated.is_empty() {
        return;
    }

    // Append newly-generated values to .env.secrets.
    let file_existed = path.exists();
    let open_result = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path);

    match open_result {
        Ok(mut f) => {
            if !file_existed {
                let _ = writeln!(
                    f,
                    "# Auto-generated by zpay-enterprise at first startup.\n\
                     # DO NOT COMMIT THIS FILE — it is gitignored.\n\
                     # LOSS = PERMANENT LOSS OF ALL ENCRYPTED WALLETS AND PRIVATE KEYS.\n\
                     # Back this file up securely (restricted, encrypted storage).",
                );
            }
            for (name, value) in &newly_generated {
                let _ = writeln!(f, "{}={}", name, value);
            }
        }
        Err(e) => {
            eprintln!(
                "zpay-enterprise: WARNING — failed to persist auto-generated \
                 secrets to {}: {}. Secrets are active for this process only; \
                 next restart will generate new values (NOT SAFE IF ANY WALLETS \
                 HAVE BEEN CREATED).",
                SECRETS_FILE, e
            );
            return;
        }
    }

    // Restrict permissions on Unix. Best-effort — not fatal if it fails.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }

    // Loud, unmissable notice so the operator knows secrets were minted.
    let names: Vec<&str> = newly_generated.iter().map(|(n, _)| *n).collect();
    eprintln!(
        "\n\
         ╔══════════════════════════════════════════════════════════════════╗\n\
         ║  zpay-enterprise: AUTO-GENERATED SECRETS                         ║\n\
         ╠══════════════════════════════════════════════════════════════════╣\n\
         ║  The following secrets were missing from the environment and    ║\n\
         ║  have been randomly generated and written to: {:<18}║\n\
         ║                                                                  ║\n\
         ║  Generated: {:<54}║\n\
         ║                                                                  ║\n\
         ║  >>> BACK UP backend/{} SECURELY. <<<             ║\n\
         ║  Loss of this file = permanent loss of all encrypted wallets.  ║\n\
         ║  Do NOT commit it (already in .gitignore).                     ║\n\
         ╚══════════════════════════════════════════════════════════════════╝\n",
        SECRETS_FILE,
        names.join(", "),
        SECRETS_FILE,
    );
}
