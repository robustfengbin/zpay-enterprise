use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::config::JwtConfig;
use crate::crypto::password::{hash_password, verify_password};
use crate::db::models::{LoginRequest, LoginResponse, User, UserResponse};
use crate::db::repositories::UserRepository;
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: i32, // user id
    pub username: String,
    pub role: String,
    pub exp: i64,
    pub iat: i64,
}

pub struct AuthService {
    user_repo: UserRepository,
    jwt_config: JwtConfig,
}

impl AuthService {
    pub fn new(user_repo: UserRepository, jwt_config: JwtConfig) -> Self {
        Self {
            user_repo,
            jwt_config,
        }
    }

    pub async fn login(&self, request: LoginRequest) -> AppResult<LoginResponse> {
        let user = self
            .user_repo
            .find_by_username(&request.username)
            .await?
            .ok_or(AppError::InvalidCredentials)?;

        if !verify_password(&request.password, &user.password_hash)? {
            return Err(AppError::InvalidCredentials);
        }

        let token = self.generate_token(&user)?;

        Ok(LoginResponse {
            token,
            user: UserResponse::from(user),
        })
    }

    pub async fn change_password(
        &self,
        user_id: i32,
        old_password: &str,
        new_password: &str,
    ) -> AppResult<()> {
        let user = self
            .user_repo
            .find_by_id(user_id)
            .await?
            .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

        if !verify_password(old_password, &user.password_hash)? {
            return Err(AppError::InvalidCredentials);
        }

        let new_hash = hash_password(new_password)?;
        self.user_repo.update_password(user_id, &new_hash).await?;

        Ok(())
    }

    pub async fn verify_user_password(&self, user_id: i32, password: &str) -> AppResult<bool> {
        let user = self
            .user_repo
            .find_by_id(user_id)
            .await?
            .ok_or_else(|| AppError::NotFound("User not found".to_string()))?;

        verify_password(password, &user.password_hash)
    }

    pub fn generate_token(&self, user: &User) -> AppResult<String> {
        let now = Utc::now();
        let expire = now + Duration::hours(self.jwt_config.expire_hours as i64);

        let claims = Claims {
            sub: user.id,
            username: user.username.clone(),
            role: user.role.clone(),
            exp: expire.timestamp(),
            iat: now.timestamp(),
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_config.secret.as_bytes()),
        )
        .map_err(|e| AppError::InternalError(format!("Token generation failed: {}", e)))
    }

    pub fn verify_token(&self, token: &str) -> AppResult<Claims> {
        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_config.secret.as_bytes()),
            &Validation::default(),
        )?;

        Ok(token_data.claims)
    }

    /// Creates the initial admin user with a caller-supplied password.
    ///
    /// The caller (main.rs) must pass the configured
    /// `security.admin_initial_password` value. Config validation guarantees
    /// the password is non-empty, at least 12 characters, and not `admin123`.
    ///
    /// Idempotent: if an admin user already exists, this is a no-op and the
    /// supplied password is ignored (existing admin's password is not modified).
    pub async fn create_default_admin(&self, initial_password: &str) -> AppResult<()> {
        let default_password = hash_password(initial_password)?;
        self.user_repo
            .create_default_admin_if_not_exists(&default_password)
            .await
    }

    pub async fn get_user(&self, user_id: i32) -> AppResult<Option<User>> {
        self.user_repo.find_by_id(user_id).await
    }
}
