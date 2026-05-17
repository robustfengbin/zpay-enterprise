//! M1 F1.1 — Auditor account + login + scope service.
//!
//! Auditors are a separate identity store from `users` — they live in their
//! own `auditors` table and use their own JWT claims (`kind = "auditor"`)
//! to keep wallet keyholders and read-only auditors strictly disjoint.
//! Token signing key is shared with the main JWT secret for v1; rotating to
//! a dedicated `WEB3_AUDITOR_JWT__SECRET` is queued for M2 hardening.

use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::config::JwtConfig;
use crate::crypto::password::{hash_password, verify_password};
use crate::db::models_m1::{
    Auditor, AuditorLoginRequest, AuditorLoginResponse, AuditorResponse, AuditorWalletScope,
    CreateAuditorRequest, CreateAuditorResponse,
};
use crate::db::repositories::AuditorRepository;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditorClaims {
    pub sub: i32, // auditor id
    pub email: String,
    /// Identity kind discriminator — must be "auditor" for tokens issued
    /// here.  Future user-issued tokens may set kind = "user" so a leaked
    /// admin token can never be replayed against /auditor/* endpoints.
    pub kind: String,
    pub exp: i64,
    pub iat: i64,
}

pub struct AuditorService {
    repo: AuditorRepository,
    jwt_config: JwtConfig,
}

impl AuditorService {
    pub fn new(repo: AuditorRepository, jwt_config: JwtConfig) -> Self {
        Self { repo, jwt_config }
    }

    // -----------------------------------------------------------------------
    // Admin-side: create / list / deactivate
    // -----------------------------------------------------------------------

    pub async fn create_auditor(
        &self,
        req: CreateAuditorRequest,
        invited_by_user_id: i32,
    ) -> AppResult<CreateAuditorResponse> {
        // Reject duplicate emails up front — UNIQUE KEY would also catch
        // this, but a typed error gives the frontend a useful message.
        if self.repo.find_by_email(&req.email).await?.is_some() {
            return Err(AppError::AlreadyExists(format!(
                "auditor with email '{}' already exists",
                req.email
            )));
        }

        if req.scope_end <= req.scope_start {
            return Err(AppError::ValidationError(
                "scope_end must be after scope_start".to_string(),
            ));
        }
        if req.wallet_ids.is_empty() {
            return Err(AppError::ValidationError(
                "at least one wallet_id is required".to_string(),
            ));
        }
        if req.max_count < 1 {
            return Err(AppError::ValidationError(
                "max_count must be >= 1".to_string(),
            ));
        }

        // Generate a 16-char alphanumeric temp password — auditor will be
        // forced to change on first login (FR-F1.1 / M1.W3).  This is a
        // one-time bootstrap secret; it never goes through Discord per
        // the [[feedback_no_secret_in_discord]] rule, the Admin shares it
        // out-of-band via their normal credential channel.
        let temp_password = generate_temp_password(16);
        let password_hash = hash_password(&temp_password)?;

        let auditor_id = self
            .repo
            .create(&req.email, &password_hash, req.name.trim(), invited_by_user_id)
            .await?;

        // Insert one scope row per wallet — keeps the data model clean even
        // if all rows share the same scope_start / scope_end / max_count.
        for wallet_id in &req.wallet_ids {
            self.repo
                .create_scope(
                    auditor_id,
                    *wallet_id,
                    invited_by_user_id,
                    req.scope_start,
                    req.scope_end,
                    req.max_count,
                )
                .await?;
        }

        // Invitation link: the frontend hosts /auditor/login; we return a URL
        // the Admin can paste / email.  Real email send is M2 (no SMTP wired
        // yet — explicit out of scope for M1).
        let invitation_link = format!("/auditor/login?email={}", urlencoding_encode(&req.email));

        Ok(CreateAuditorResponse {
            auditor_id,
            invitation_link,
            temp_password,
        })
    }

    pub async fn list_auditors(&self) -> AppResult<Vec<AuditorResponse>> {
        let rows = self.repo.list_all().await?;
        Ok(rows.into_iter().map(AuditorResponse::from).collect())
    }

    pub async fn deactivate(&self, id: i32) -> AppResult<()> {
        self.repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("auditor {} not found", id)))?;
        self.repo.set_active(id, false).await
    }

    // -----------------------------------------------------------------------
    // Auditor-side: login / token / me / scopes
    // -----------------------------------------------------------------------

    pub async fn login(&self, req: AuditorLoginRequest) -> AppResult<AuditorLoginResponse> {
        let auditor = self
            .repo
            .find_by_email(&req.email)
            .await?
            .ok_or(AppError::InvalidCredentials)?;

        if !auditor.active {
            return Err(AppError::Forbidden(
                "auditor account is deactivated".to_string(),
            ));
        }

        if !verify_password(&req.password, &auditor.password_hash)? {
            return Err(AppError::InvalidCredentials);
        }

        let token = self.generate_token(&auditor)?;
        // Best-effort update — don't fail login just because of a stat update.
        let _ = self.repo.update_last_login(auditor.id).await;

        Ok(AuditorLoginResponse {
            token,
            auditor: AuditorResponse::from(auditor),
        })
    }

    pub fn generate_token(&self, auditor: &Auditor) -> AppResult<String> {
        let now = Utc::now();
        let expire = now + Duration::hours(self.jwt_config.expire_hours as i64);

        let claims = AuditorClaims {
            sub: auditor.id,
            email: auditor.email.clone(),
            kind: "auditor".to_string(),
            exp: expire.timestamp(),
            iat: now.timestamp(),
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_config.secret.as_bytes()),
        )
        .map_err(|e| AppError::InternalError(format!("auditor token gen failed: {}", e)))
    }

    pub fn verify_token(&self, token: &str) -> AppResult<AuditorClaims> {
        let data = decode::<AuditorClaims>(
            token,
            &DecodingKey::from_secret(self.jwt_config.secret.as_bytes()),
            &Validation::default(),
        )?;
        // Hard-guard against a user-issued JWT being replayed against
        // /auditor/* endpoints: a user Claims struct decodes here only if it
        // happens to have a `kind` field, but we reject any token whose kind
        // is not "auditor".  Belt-and-braces alongside the separate /auditor
        // route prefix.
        if data.claims.kind != "auditor" {
            return Err(AppError::Unauthorized(
                "token is not an auditor token".to_string(),
            ));
        }
        Ok(data.claims)
    }

    pub async fn find_by_id(&self, id: i32) -> AppResult<Option<Auditor>> {
        self.repo.find_by_id(id).await
    }

    pub async fn list_scopes(&self, auditor_id: i32) -> AppResult<Vec<AuditorWalletScope>> {
        self.repo.list_scopes_for_auditor(auditor_id).await
    }

    pub async fn assert_wallet_in_scope(
        &self,
        auditor_id: i32,
        wallet_id: i32,
    ) -> AppResult<AuditorWalletScope> {
        let now = Utc::now();
        self.repo
            .find_active_scope(auditor_id, wallet_id, now)
            .await?
            .ok_or_else(|| {
                AppError::Forbidden(format!(
                    "auditor {} has no active scope for wallet {}",
                    auditor_id, wallet_id
                ))
            })
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn generate_temp_password(len: usize) -> String {
    // Mixed-case + digit alphabet; avoids ambiguous chars (0/O, 1/l/I)
    // so the Admin can read it out / paste it without typos.
    const ALPHA: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789";
    let mut rng = rand::thread_rng();
    (0..len)
        .map(|_| {
            let i = rng.gen_range(0..ALPHA.len());
            ALPHA[i] as char
        })
        .collect()
}

fn urlencoding_encode(s: &str) -> String {
    // Minimal percent-encoding for query string value; we only need '@' '+' ' '.
    // Pulling in a full URL encoding crate would be over-engineering per C-5.
    s.replace('@', "%40").replace('+', "%2B").replace(' ', "%20")
}
