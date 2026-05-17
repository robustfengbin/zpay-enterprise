//! M1 F1.1 — ViewingKey export service.
//!
//! Derives Orchard viewing keys (OVK / IVK / UFVK) from an existing Zcash
//! wallet, encrypts the serialized key, and stores it under a one-time
//! download token with a 24-hour TTL.
//!
//! Security model (per F1.1 §3.1 / NFR-1 / FR-1.1.11):
//!   * Caller supplies user password to prove the active session has fresh
//!     intent (prevents a leaked JWT alone from exfiltrating viewing keys).
//!   * Serialized viewing key is AES-GCM encrypted with the global
//!     ENCRYPTION_KEY before insert.  The plaintext never sits at rest.
//!   * download_token is 32 random bytes (base64url) — separate from the
//!     JWT so the Admin can hand it off via any secure channel.
//!   * `claim_for_download` is the one-time-download enforcement: an
//!     atomic UPDATE marks the row downloaded AND zeroes the payload in
//!     the same statement, so a concurrent retry sees an already-claimed
//!     row and gets 410 Gone.
//!   * SHA-256 hash of the plaintext key is stored independently so the
//!     full audit trail is reconstructable even after payload is zeroed.

use std::sync::Arc;

use orchard::keys::Scope;
use rand::RngCore;
use sha2::{Digest, Sha256};
use zcash_address::unified::{Encoding as _, Fvk, Ufvk};
use zcash_protocol::consensus::NetworkType;

use crate::blockchain::zcash::orchard::keys::OrchardKeyManager;
use crate::config::SecurityConfig;
use crate::crypto::encryption::{decrypt, encrypt};
use crate::db::models_m1::{ExportViewingKeyRequest, ExportViewingKeyResponse};
use crate::db::repositories::{ViewingKeyExportRepository, WalletRepository};
use crate::error::{AppError, AppResult};
use crate::services::AuthService;

pub struct ViewingKeyService {
    wallet_repo: WalletRepository,
    export_repo: ViewingKeyExportRepository,
    auth_service: Arc<AuthService>,
    security_config: SecurityConfig,
}

impl ViewingKeyService {
    pub fn new(
        wallet_repo: WalletRepository,
        export_repo: ViewingKeyExportRepository,
        auth_service: Arc<AuthService>,
        security_config: SecurityConfig,
    ) -> Self {
        Self {
            wallet_repo,
            export_repo,
            auth_service,
            security_config,
        }
    }

    pub async fn export(
        &self,
        wallet_id: i32,
        user_id: i32,
        req: ExportViewingKeyRequest,
    ) -> AppResult<ExportViewingKeyResponse> {
        // 1. Validate key_type up front (avoids decrypting wallet for a
        // doomed request).
        let key_type = req.key_type.to_lowercase();
        match key_type.as_str() {
            "ovk" | "ivk" | "ufvk" => {}
            other => {
                return Err(AppError::ValidationError(format!(
                    "invalid key_type '{}'; expected ovk | ivk | ufvk",
                    other
                )));
            }
        }

        // 2. Re-verify user password — fresh intent check.
        if !self
            .auth_service
            .verify_user_password(user_id, &req.password)
            .await?
        {
            return Err(AppError::InvalidCredentials);
        }

        // 3. Wallet must exist + be Zcash (Orchard keys do not exist for ETH).
        let wallet = self
            .wallet_repo
            .find_by_id(wallet_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("wallet {} not found", wallet_id)))?;
        if wallet.chain != "zcash" {
            return Err(AppError::ValidationError(format!(
                "viewing key export only supported for zcash wallets (this is {})",
                wallet.chain
            )));
        }

        // 4. Decrypt wallet private key + derive Orchard viewing key.
        let private_key_hex = decrypt(
            &wallet.encrypted_private_key,
            &self.security_config.encryption_key,
        )?;
        let birthday = wallet.orchard_birthday_height.unwrap_or(0);
        let (_sk, vk) = OrchardKeyManager::derive_from_private_key(
            &private_key_hex,
            0, // account_index — matches the wallet's wallet_service derivation
            birthday,
        )
        .map_err(|e| AppError::InternalError(format!("orchard derivation failed: {:?}", e)))?;

        // 5. Serialize requested key as hex.  Standard ZIP-316 unified
        // string encoding is M2 polish; M1 ships hex with metadata header
        // so audit consumers can identify key kind + wallet birthday.
        let plaintext = match key_type.as_str() {
            "ovk" => {
                let ovk = vk.fvk().to_ovk(Scope::External);
                let bytes: &[u8] = ovk.as_ref();
                format!(
                    "orchard-ovk:account={}:birthday={}:hex={}",
                    vk.account_index,
                    vk.birthday_height,
                    hex::encode(bytes)
                )
            }
            "ivk" => {
                let ivk = vk.fvk().to_ivk(Scope::External);
                let bytes = ivk.to_bytes();
                format!(
                    "orchard-ivk:account={}:birthday={}:hex={}",
                    vk.account_index,
                    vk.birthday_height,
                    hex::encode(bytes)
                )
            }
            "ufvk" => {
                // ZIP-316 standard Unified FVK string ("uview..." on mainnet)
                // so the auditor can paste this straight into Zashi / any
                // ZIP-316–compatible viewing-only wallet without any custom
                // header parsing.  We still prepend a comment line with
                // account + birthday so audit consumers retain the metadata
                // they need to bootstrap a wallet (birthday) and confirm
                // provenance — Zashi ignores anything before the actual
                // `uview` token on a single-line paste.
                let bytes = vk.fvk().to_bytes();
                let arr: [u8; 96] = bytes.as_slice().try_into().map_err(|_| {
                    AppError::InternalError(format!(
                        "expected Orchard FVK to be 96 bytes, got {}",
                        bytes.len()
                    ))
                })?;
                let ufvk = Ufvk::try_from_items(vec![Fvk::Orchard(arr)]).map_err(|e| {
                    AppError::InternalError(format!("ZIP-316 UFVK assembly failed: {:?}", e))
                })?;
                let encoded = ufvk.encode(&NetworkType::Main);
                format!(
                    "# orchard-ufvk account={} birthday={}\n{}",
                    vk.account_index, vk.birthday_height, encoded
                )
            }
            _ => unreachable!("key_type validated above"),
        };

        // 6. Hash plaintext before encrypting — audit log can later prove
        // which key was exported even after the payload is zeroed.
        let payload_hash = {
            let mut hasher = Sha256::new();
            hasher.update(plaintext.as_bytes());
            hex::encode(hasher.finalize())
        };

        // 7. Encrypt payload at rest with the same ENCRYPTION_KEY used for
        // wallet private keys.  encrypt() returns base64; we store bytes
        // for consistency with the BLOB column.
        let encrypted_b64 = encrypt(&plaintext, &self.security_config.encryption_key)?;
        let encrypted_payload = encrypted_b64.into_bytes();

        // 8. Random 32-byte token, base64url so it survives URL transit.
        let download_token = generate_download_token();

        let (export_id, expires_at) = self
            .export_repo
            .create(
                wallet_id,
                user_id,
                &key_type,
                &encrypted_payload,
                &payload_hash,
                &download_token,
            )
            .await?;

        tracing::info!(
            "[viewing_key] exported {} for wallet={} by user={} export_id={}",
            key_type,
            wallet_id,
            user_id,
            export_id
        );

        Ok(ExportViewingKeyResponse {
            export_id,
            download_token,
            expires_at,
        })
    }

    /// Returns the plaintext viewing key body on first call only.
    /// Subsequent calls return 410 Gone (token consumed) — frontend should
    /// surface this clearly so the Admin re-issues an export if the
    /// auditor never received the original.
    pub async fn download(&self, token: &str, ip: &str) -> AppResult<String> {
        let row = self
            .export_repo
            .find_by_token(token)
            .await?
            .ok_or_else(|| AppError::NotFound("download token not found".to_string()))?;

        if row.downloaded_at.is_some() {
            return Err(AppError::Forbidden(
                "this viewing key has already been downloaded; create a new export"
                    .to_string(),
            ));
        }
        if row.expires_at < chrono::Utc::now() {
            return Err(AppError::Forbidden(
                "this download token has expired (>24h since export)".to_string(),
            ));
        }

        // Atomic claim — wins over concurrent requests, fails if anyone
        // else already claimed.
        let claimed = self.export_repo.claim_for_download(row.id, ip).await?;
        if !claimed {
            return Err(AppError::Forbidden(
                "download token was claimed concurrently — re-issue export".to_string(),
            ));
        }

        // Decrypt body from the in-memory copy (the row's payload was
        // zeroed by `claim_for_download` already).
        let encrypted_b64 = String::from_utf8(row.encrypted_payload).map_err(|_| {
            AppError::InternalError("viewing_key_exports.encrypted_payload not valid utf8".into())
        })?;
        let plaintext = decrypt(&encrypted_b64, &self.security_config.encryption_key)?;

        tracing::info!(
            "[viewing_key] downloaded export={} by ip={} (one-time)",
            row.id,
            ip
        );

        Ok(plaintext)
    }

    pub async fn list_for_wallet(
        &self,
        wallet_id: i32,
    ) -> AppResult<Vec<crate::db::models_m1::ViewingKeyExport>> {
        self.export_repo.list_by_wallet(wallet_id).await
    }
}

fn generate_download_token() -> String {
    // 32 bytes -> 43 chars base64url (no padding) — comfortably fits the
    // VARCHAR(64) column and is trivially URL-safe.
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    base64_url_encode(&bytes)
}

fn base64_url_encode(input: &[u8]) -> String {
    // Use the standard base64 crate via the alphabet shipped with the
    // project (base64 0.21 is already in Cargo.toml for other modules).
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    URL_SAFE_NO_PAD.encode(input)
}
