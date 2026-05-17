//! M1 F2.1 — Approval Policy + Decision service.
//!
//! M1 W1 scope: Policy CRUD + matching lookup.
//! M1 W2+ scope: ApprovalDecision (approve / reject) — wires into
//! TransferService state machine and is filled in next increment.

use std::str::FromStr;
use std::sync::Arc;

use rust_decimal::Decimal;
use serde_json::json;
use sqlx::MySqlPool;

use crate::blockchain::ChainRegistry;
use crate::db::models::Transfer;
use crate::db::models_m1::{
    ApprovalDecisionRequest, ApprovalPolicy, CreateApprovalPolicyRequest, TransferApproval,
};
use crate::db::repositories::{
    ApprovalPolicyRepository, TransferApprovalRepository, TransferRepository,
};
use crate::error::{AppError, AppResult};

pub struct ApprovalService {
    policy_repo: ApprovalPolicyRepository,
    approval_repo: TransferApprovalRepository,
    transfer_repo: TransferRepository,
    chain_registry: Arc<ChainRegistry>,
    pool: MySqlPool,
}

impl ApprovalService {
    pub fn new(
        policy_repo: ApprovalPolicyRepository,
        approval_repo: TransferApprovalRepository,
        transfer_repo: TransferRepository,
        chain_registry: Arc<ChainRegistry>,
        pool: MySqlPool,
    ) -> Self {
        Self {
            policy_repo,
            approval_repo,
            transfer_repo,
            chain_registry,
            pool,
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

    // -----------------------------------------------------------------------
    // Approval decisions (approve / reject a pending transfer)
    //
    // M1.W1 scope: the decision is written + transfer.status flips to
    // approved | rejected.  Wiring the *creation* of an awaiting_approval
    // transfer (policy-driven status pivot in /transfers POST) is the
    // companion change inside transfer_service — slated for the next
    // increment so this commit stays additive.
    // -----------------------------------------------------------------------

    pub async fn approve(
        &self,
        transfer_id: i32,
        approver_user_id: i32,
        req: ApprovalDecisionRequest,
    ) -> AppResult<ApprovalDecisionResult> {
        self.record_decision(transfer_id, approver_user_id, req, "approve")
            .await
    }

    pub async fn reject(
        &self,
        transfer_id: i32,
        approver_user_id: i32,
        req: ApprovalDecisionRequest,
    ) -> AppResult<ApprovalDecisionResult> {
        // PRD §2.3 FR-9 — rejections must carry a reason; we enforce it
        // up front rather than letting an empty string land in audit.
        if req
            .reason
            .as_deref()
            .map(|s| s.trim().len() < 5)
            .unwrap_or(true)
        {
            return Err(AppError::ValidationError(
                "reject requires reason >= 5 characters".to_string(),
            ));
        }
        self.record_decision(transfer_id, approver_user_id, req, "reject")
            .await
    }

    async fn record_decision(
        &self,
        transfer_id: i32,
        approver_user_id: i32,
        req: ApprovalDecisionRequest,
        decision: &str,
    ) -> AppResult<ApprovalDecisionResult> {
        // 1. Idempotency replay short-circuit — if the same key was used
        // before, return the prior decision rather than re-executing.
        if let Some(key) = req.idempotency_key.as_deref() {
            if let Some(prev) = self.approval_repo.find_by_idempotency_key(key).await? {
                let transfer = self
                    .transfer_repo
                    .find_by_id(prev.transfer_id)
                    .await?
                    .ok_or_else(|| {
                        AppError::InternalError(format!(
                            "transfer {} referenced by idempotency key gone",
                            prev.transfer_id
                        ))
                    })?;
                return Ok(ApprovalDecisionResult {
                    transfer,
                    approval: prev,
                    replayed: true,
                });
            }
        }

        // 2. Transfer must exist and be in a decidable state.  M1.W1 accepts
        // either `awaiting_approval` (the future state once /transfers POST
        // is wired) or `pending` (backward-compat for transfers created
        // before the state-machine change ships).
        let transfer = self
            .transfer_repo
            .find_by_id(transfer_id)
            .await?
            .ok_or_else(|| AppError::NotFound(format!("transfer {} not found", transfer_id)))?;

        if !matches!(transfer.status.as_str(), "awaiting_approval" | "pending") {
            return Err(AppError::ValidationError(format!(
                "transfer {} is in state '{}' and cannot be {}d",
                transfer_id, transfer.status, decision
            )));
        }

        // 3. Maker ≠ checker — defended at the DB layer too via
        // uq_one_decision_per_approver, but a typed app-level rejection
        // gives the frontend a much better error to render.
        if transfer.initiated_by == approver_user_id {
            return Err(AppError::Forbidden(
                "maker may not approve their own transfer (maker ≠ checker)".to_string(),
            ));
        }

        // 4. Snapshot the policy that was in effect at the time of decision
        // so the audit trail is self-contained even if policy is later edited.
        let policy_snapshot = self
            .matching_policy_for(
                &transfer.chain,
                &transfer.token,
                transfer.amount,
                transfer.wallet_id,
                transfer.initiated_by,
            )
            .await?
            .map(|p| {
                json!({
                    "policy_id": p.id,
                    "scope": p.scope,
                    "scope_id": p.scope_id,
                    "chain": p.chain,
                    "token": p.token,
                    "amount_threshold": p.amount_threshold.to_string(),
                    "sla_minutes": p.sla_minutes,
                    "required_count": p.required_count,
                })
            });

        // 5. Insert the approval row.  DB unique constraints catch:
        //    - duplicate idempotency_key (uq_approval_idem)
        //    - same approver decided twice (uq_one_decision_per_approver)
        let approval_id = self
            .approval_repo
            .create(
                transfer_id,
                approver_user_id,
                decision,
                req.reason.as_deref(),
                policy_snapshot.as_ref(),
                req.idempotency_key.as_deref(),
            )
            .await?;

        // 6. Flip the transfer status.  required_count > 1 (N-of-M policies)
        // is M2 (F2.6); M1 treats one approver as final.
        let new_status = if decision == "approve" { "approved" } else { "rejected" };
        self.transfer_repo
            .update_status(transfer_id, new_status, None, None)
            .await?;

        // Re-fetch so the response reflects the post-decision state.
        let transfer = self
            .transfer_repo
            .find_by_id(transfer_id)
            .await?
            .ok_or_else(|| {
                AppError::InternalError(format!("transfer {} vanished after update", transfer_id))
            })?;

        let approval = self
            .approval_repo
            .list_by_transfer(transfer_id)
            .await?
            .into_iter()
            .find(|a| a.id == approval_id)
            .ok_or_else(|| {
                AppError::InternalError("approval row vanished after insert".to_string())
            })?;

        Ok(ApprovalDecisionResult {
            transfer,
            approval,
            replayed: false,
        })
    }

    pub async fn list_approvals_for_transfer(
        &self,
        transfer_id: i32,
    ) -> AppResult<Vec<TransferApproval>> {
        self.approval_repo.list_by_transfer(transfer_id).await
    }

    /// Listing pending-approval transfers is a cross-table read that does
    /// not fit any of the existing repositories cleanly, so we run it
    /// inline against the shared pool rather than spawning a new repo.
    pub async fn list_pending_for_user(&self, _viewer_user_id: i32) -> AppResult<Vec<Transfer>> {
        let rows = sqlx::query_as::<_, Transfer>(
            "SELECT * FROM transfers WHERE status = 'awaiting_approval' ORDER BY created_at ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}

pub struct ApprovalDecisionResult {
    pub transfer: Transfer,
    pub approval: TransferApproval,
    pub replayed: bool,
}

impl serde::Serialize for ApprovalDecisionResult {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut st = s.serialize_struct("ApprovalDecisionResult", 3)?;
        st.serialize_field("transfer", &self.transfer)?;
        st.serialize_field("approval", &self.approval)?;
        st.serialize_field("replayed", &self.replayed)?;
        st.end()
    }
}
