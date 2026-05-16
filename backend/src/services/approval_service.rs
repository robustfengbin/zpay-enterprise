//! M1 F2.1 — Approval Policy + Decision service.
//!
//! M1 W1 scope: Policy CRUD + matching lookup.
//! M1 W2+ scope: ApprovalDecision (approve / reject) — wires into
//! TransferService state machine and is filled in next increment.

use std::str::FromStr;
use std::sync::Arc;

use rust_decimal::Decimal;

use crate::blockchain::ChainRegistry;
use crate::db::models_m1::{ApprovalPolicy, CreateApprovalPolicyRequest};
use crate::db::repositories::ApprovalPolicyRepository;
use crate::error::{AppError, AppResult};

pub struct ApprovalService {
    policy_repo: ApprovalPolicyRepository,
    chain_registry: Arc<ChainRegistry>,
}

impl ApprovalService {
    pub fn new(
        policy_repo: ApprovalPolicyRepository,
        chain_registry: Arc<ChainRegistry>,
    ) -> Self {
        Self {
            policy_repo,
            chain_registry,
        }
    }

    // -----------------------------------------------------------------------
    // Policy CRUD
    // -----------------------------------------------------------------------

    pub async fn create_policy(
        &self,
        req: CreateApprovalPolicyRequest,
        created_by: i32,
    ) -> AppResult<ApprovalPolicy> {
        // 1. scope validation
        match req.scope.as_str() {
            "global" => {
                if req.scope_id.is_some() {
                    return Err(AppError::ValidationError(
                        "scope=global must not have scope_id".to_string(),
                    ));
                }
            }
            "wallet" | "user" => {
                if req.scope_id.is_none() {
                    return Err(AppError::ValidationError(format!(
                        "scope={} requires scope_id",
                        req.scope
                    )));
                }
            }
            other => {
                return Err(AppError::ValidationError(format!(
                    "invalid scope '{}'; expected 'global' | 'wallet' | 'user'",
                    other
                )));
            }
        }

        // 2. chain must be registered (ethereum / zcash)
        let _ = self.chain_registry.get(&req.chain)?;

        // 3. token non-empty
        let token = req.token.trim().to_uppercase();
        if token.is_empty() {
            return Err(AppError::ValidationError("token cannot be empty".to_string()));
        }

        // 4. amount_threshold > 0 parse
        let threshold = Decimal::from_str(&req.amount_threshold).map_err(|e| {
            AppError::ValidationError(format!("invalid amount_threshold: {}", e))
        })?;
        if threshold <= Decimal::ZERO {
            return Err(AppError::ValidationError(
                "amount_threshold must be positive".to_string(),
            ));
        }

        // 5. SLA + required_count sanity
        if req.sla_minutes < 5 {
            return Err(AppError::ValidationError(
                "sla_minutes must be >= 5".to_string(),
            ));
        }
        if !(1..=10).contains(&req.required_count) {
            return Err(AppError::ValidationError(
                "required_count must be 1..=10".to_string(),
            ));
        }

        let id = self
            .policy_repo
            .create(
                &req.scope,
                req.scope_id,
                &req.chain,
                &token,
                threshold,
                req.sla_minutes,
                req.required_count,
                req.enabled,
                created_by,
            )
            .await?;

        self.policy_repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| AppError::InternalError("policy vanished after insert".to_string()))
    }

    pub async fn list_policies(&self) -> AppResult<Vec<ApprovalPolicy>> {
        self.policy_repo.list_all().await
    }

    pub async fn delete_policy(&self, id: i32) -> AppResult<()> {
        // 404 if not present, rather than silently no-op.
        self.policy_repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("approval_policy {} not found", id)))?;
        self.policy_repo.delete(id).await
    }

    pub async fn set_policy_enabled(&self, id: i32, enabled: bool) -> AppResult<()> {
        self.policy_repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("approval_policy {} not found", id)))?;
        self.policy_repo.set_enabled(id, enabled).await
    }

    // -----------------------------------------------------------------------
    // Policy matching — used by the transfer flow to decide whether a
    // transfer needs approval.  Returns the *narrowest* matching policy
    // (the one with the smallest threshold that the amount actually meets).
    // -----------------------------------------------------------------------

    /// Returns Some(policy) if this transfer should be gated; None if it can
    /// proceed directly.  Caller passes the canonical token symbol (uppercase).
    pub async fn matching_policy_for(
        &self,
        chain: &str,
        token: &str,
        amount: Decimal,
        wallet_id: i32,
        user_id: i32,
    ) -> AppResult<Option<ApprovalPolicy>> {
        let candidates = self
            .policy_repo
            .find_matching(chain, token, wallet_id, user_id)
            .await?;

        // candidates are already ordered most-specific-first then by threshold asc;
        // pick the first policy whose threshold <= amount.
        Ok(candidates.into_iter().find(|p| amount >= p.amount_threshold))
    }
}
