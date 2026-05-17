//! M1 2026-06 HTTP handlers — stub layer for F1.1 / F2.1 / F3.1.
//!
//! Each handler returns a well-typed empty / mock response so the frontend
//! can wire up axios clients and verify the contract.  Business logic is
//! filled in incrementally — see the per-feature PRD §5 + §7 milestones.
//!
//! All handlers are additive — existing /auth /wallets /transfers handlers
//! in this directory are NOT modified (per Robust's 2026-05-16 rule).

use std::sync::Arc;

use actix_web::{web, HttpResponse};
use serde_json::json;

use actix_web::{HttpMessage, HttpRequest};

use crate::api::middleware::AuthenticatedUser;
use crate::db::repositories::WalletRepository;
use crate::error::{AppError, AppResult};
use crate::services::auditor_service::AuditorClaims;
use crate::services::{
    ApprovalService, AuditorService, EmployeeService, PayrollService, PaymentDisclosureService,
    ViewingKeyService,
};

// ===========================================================================
// F1.1 — Viewing Key audit (Admin side)
// ===========================================================================

pub async fn export_viewing_key(
    path: web::Path<i32>,
    body: web::Json<crate::db::models_m1::ExportViewingKeyRequest>,
    viewing_key_service: web::Data<Arc<ViewingKeyService>>,
    user: AuthenticatedUser,
) -> AppResult<HttpResponse> {
    let resp = viewing_key_service
        .export(path.into_inner(), user.user_id, body.into_inner())
        .await?;
    Ok(HttpResponse::Created().json(resp))
}

pub async fn list_viewing_key_exports(
    path: web::Path<i32>,
    viewing_key_service: web::Data<Arc<ViewingKeyService>>,
) -> AppResult<HttpResponse> {
    let exports = viewing_key_service
        .list_for_wallet(path.into_inner())
        .await?;
    // Map to response DTO that hides encrypted_payload + download_token from
    // the listing view — those only come back from the create call.
    let trimmed: Vec<_> = exports
        .into_iter()
        .map(|e| {
            json!({
                "id": e.id,
                "wallet_id": e.wallet_id,
                "key_type": e.key_type,
                "downloaded_at": e.downloaded_at,
                "expires_at": e.expires_at,
                "created_at": e.created_at,
            })
        })
        .collect();
    Ok(HttpResponse::Ok().json(trimmed))
}

pub async fn download_viewing_key(
    token: web::Path<String>,
    req: HttpRequest,
    viewing_key_service: web::Data<Arc<ViewingKeyService>>,
) -> AppResult<HttpResponse> {
    let ip = req
        .peer_addr()
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let body = viewing_key_service
        .download(&token.into_inner(), &ip)
        .await?;
    Ok(HttpResponse::Ok()
        .content_type("text/plain; charset=utf-8")
        .body(body))
}

pub async fn create_auditor(
    body: web::Json<crate::db::models_m1::CreateAuditorRequest>,
    auditor_service: web::Data<Arc<AuditorService>>,
    user: AuthenticatedUser,
) -> AppResult<HttpResponse> {
    let resp = auditor_service
        .create_auditor(body.into_inner(), user.user_id)
        .await?;
    Ok(HttpResponse::Created().json(resp))
}

pub async fn list_auditors(
    auditor_service: web::Data<Arc<AuditorService>>,
) -> AppResult<HttpResponse> {
    let auditors = auditor_service.list_auditors().await?;
    Ok(HttpResponse::Ok().json(auditors))
}

pub async fn deactivate_auditor(
    path: web::Path<i32>,
    auditor_service: web::Data<Arc<AuditorService>>,
) -> AppResult<HttpResponse> {
    auditor_service.deactivate(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub async fn create_payment_disclosure(
    path: web::Path<i32>,
    body: web::Json<crate::db::models_m1::CreatePaymentDisclosureRequest>,
    disclosure_service: web::Data<Arc<PaymentDisclosureService>>,
    user: AuthenticatedUser,
) -> AppResult<HttpResponse> {
    let row = disclosure_service
        .generate(path.into_inner(), user.user_id, body.into_inner())
        .await?;
    Ok(HttpResponse::Accepted().json(json!({
        "disclosure_id": row.id,
        "status": row.status,
        "tx_count": row.tx_count,
        "created_at": row.created_at,
        "expires_at": row.expires_at,
    })))
}

pub async fn get_payment_disclosure(
    path: web::Path<i32>,
    disclosure_service: web::Data<Arc<PaymentDisclosureService>>,
) -> AppResult<HttpResponse> {
    let row = disclosure_service.get(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(row))
}

pub async fn download_payment_disclosure(
    path: web::Path<i32>,
    disclosure_service: web::Data<Arc<PaymentDisclosureService>>,
) -> AppResult<HttpResponse> {
    let row = disclosure_service.get(path.into_inner()).await?;
    if row.status != "ready" {
        return Err(AppError::ValidationError(format!(
            "disclosure {} not ready (status={})",
            row.id, row.status
        )));
    }
    // M1.W1 — return the JSON body directly with format-tagged content type.
    // PDF / CSV serialization is M1.W2 when the real disclosure body lands.
    let body = row
        .disclosure_json
        .unwrap_or_else(|| json!({"error": "no body"}));
    Ok(HttpResponse::Ok()
        .content_type("application/json")
        .json(body))
}

pub async fn list_payment_disclosures(
    path: web::Path<i32>,
    disclosure_service: web::Data<Arc<PaymentDisclosureService>>,
) -> AppResult<HttpResponse> {
    let rows = disclosure_service
        .list_by_wallet(path.into_inner())
        .await?;
    Ok(HttpResponse::Ok().json(rows))
}

// ===========================================================================
// F1.1 — Auditor side (independent prefix /api/v1/auditor/*)
// Token guard is inline (no separate AuditorAuthMiddleware in M1.W1 —
// scheduled for W2 once ViewingKey + Disclosure flows ship).
// ===========================================================================

/// Extract + verify auditor JWT from Authorization header.  Returns 401 on
/// missing / invalid / wrong-kind tokens.
fn require_auditor(
    req: &HttpRequest,
    auditor_service: &AuditorService,
) -> AppResult<AuditorClaims> {
    let header = req
        .headers()
        .get(actix_web::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| AppError::Unauthorized("missing Authorization header".to_string()))?;

    let token = header
        .strip_prefix("Bearer ")
        .ok_or_else(|| AppError::Unauthorized("malformed Authorization header".to_string()))?;

    let claims = auditor_service.verify_token(token)?;
    // Stash claims into request extensions for downstream handlers in the
    // same call, matching the AuthMiddleware pattern.
    req.extensions_mut().insert(claims.clone());
    Ok(claims)
}

pub async fn auditor_login(
    body: web::Json<crate::db::models_m1::AuditorLoginRequest>,
    auditor_service: web::Data<Arc<AuditorService>>,
) -> AppResult<HttpResponse> {
    let resp = auditor_service.login(body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(resp))
}

pub async fn auditor_me(
    req: HttpRequest,
    auditor_service: web::Data<Arc<AuditorService>>,
) -> AppResult<HttpResponse> {
    let claims = require_auditor(&req, &auditor_service)?;
    let auditor = auditor_service
        .find_by_id(claims.sub)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("auditor {} not found", claims.sub)))?;
    let scopes = auditor_service.list_scopes(claims.sub).await?;
    Ok(HttpResponse::Ok().json(json!({
        "auditor": crate::db::models_m1::AuditorResponse::from(auditor),
        "scopes": scopes,
    })))
}

pub async fn auditor_list_wallets(
    req: HttpRequest,
    auditor_service: web::Data<Arc<AuditorService>>,
    wallet_repo: web::Data<WalletRepository>,
) -> AppResult<HttpResponse> {
    let claims = require_auditor(&req, &auditor_service)?;
    let scopes = auditor_service.list_scopes(claims.sub).await?;

    // Look up each wallet for the auditor's scopes.  Wallets are still the
    // authoritative source — scope only carries the wallet_id + time window.
    let mut wallets = Vec::with_capacity(scopes.len());
    for scope in scopes {
        if let Some(w) = wallet_repo.find_by_id(scope.wallet_id).await? {
            wallets.push(json!({
                "wallet_id": w.id,
                "address": w.address,
                "chain": w.chain,
                "scope_start": scope.scope_start_ts,
                "scope_end": scope.scope_end_ts,
                "max_disclosure_count": scope.max_disclosure_count,
                "current_count": scope.current_count,
            }));
        }
    }
    Ok(HttpResponse::Ok().json(wallets))
}

pub async fn auditor_wallet_balance(
    req: HttpRequest,
    path: web::Path<i32>,
    auditor_service: web::Data<Arc<AuditorService>>,
) -> AppResult<HttpResponse> {
    let claims = require_auditor(&req, &auditor_service)?;
    auditor_service
        .assert_wallet_in_scope(claims.sub, path.into_inner())
        .await?;
    // Real balance read requires the wallet's viewing key to be decrypted —
    // deferred to W2 (paired with ViewingKey export real impl).  Return a
    // typed stub so the frontend can render the page; not a security gap
    // because we have already enforced scope and auditor identity above.
    Ok(HttpResponse::Ok().json(json!({
        "native_balance": "0",
        "tokens": [],
        "stub": true,
        "note": "wallet balance read via viewing key — wired in M1.W2",
    })))
}

pub async fn auditor_wallet_transfers(
    req: HttpRequest,
    path: web::Path<i32>,
    auditor_service: web::Data<Arc<AuditorService>>,
) -> AppResult<HttpResponse> {
    let claims = require_auditor(&req, &auditor_service)?;
    auditor_service
        .assert_wallet_in_scope(claims.sub, path.into_inner())
        .await?;
    // Real transfers query (filtered by scope_start..scope_end) wired in W2.
    Ok(HttpResponse::Ok().json(json!([])))
}

pub async fn auditor_wallet_disclosures(
    req: HttpRequest,
    path: web::Path<i32>,
    auditor_service: web::Data<Arc<AuditorService>>,
) -> AppResult<HttpResponse> {
    let claims = require_auditor(&req, &auditor_service)?;
    auditor_service
        .assert_wallet_in_scope(claims.sub, path.into_inner())
        .await?;
    Ok(HttpResponse::Ok().json(json!([])))
}

// ===========================================================================
// F2.1 — Maker / Checker dual-sign
// ===========================================================================

pub async fn approve_transfer(
    path: web::Path<i32>,
    body: web::Json<crate::db::models_m1::ApprovalDecisionRequest>,
    approval_service: web::Data<Arc<ApprovalService>>,
    user: AuthenticatedUser,
) -> AppResult<HttpResponse> {
    let result = approval_service
        .approve(path.into_inner(), user.user_id, body.into_inner())
        .await?;
    Ok(HttpResponse::Ok().json(result))
}

pub async fn reject_transfer(
    path: web::Path<i32>,
    body: web::Json<crate::db::models_m1::ApprovalDecisionRequest>,
    approval_service: web::Data<Arc<ApprovalService>>,
    user: AuthenticatedUser,
) -> AppResult<HttpResponse> {
    let result = approval_service
        .reject(path.into_inner(), user.user_id, body.into_inner())
        .await?;
    Ok(HttpResponse::Ok().json(result))
}

pub async fn list_approvals_for_transfer(
    path: web::Path<i32>,
    approval_service: web::Data<Arc<ApprovalService>>,
) -> AppResult<HttpResponse> {
    let approvals = approval_service
        .list_approvals_for_transfer(path.into_inner())
        .await?;
    Ok(HttpResponse::Ok().json(approvals))
}

pub async fn list_pending_approvals(
    approval_service: web::Data<Arc<ApprovalService>>,
    user: AuthenticatedUser,
) -> AppResult<HttpResponse> {
    let transfers = approval_service.list_pending_for_user(user.user_id).await?;
    Ok(HttpResponse::Ok().json(transfers))
}

pub async fn list_approval_policies(
    approval_service: web::Data<Arc<ApprovalService>>,
) -> AppResult<HttpResponse> {
    let policies = approval_service.list_policies().await?;
    Ok(HttpResponse::Ok().json(policies))
}

pub async fn create_approval_policy(
    body: web::Json<crate::db::models_m1::CreateApprovalPolicyRequest>,
    approval_service: web::Data<Arc<ApprovalService>>,
    user: AuthenticatedUser,
) -> AppResult<HttpResponse> {
    let policy = approval_service
        .create_policy(body.into_inner(), user.user_id)
        .await?;
    Ok(HttpResponse::Created().json(policy))
}

pub async fn delete_approval_policy(
    path: web::Path<i32>,
    approval_service: web::Data<Arc<ApprovalService>>,
) -> AppResult<HttpResponse> {
    approval_service.delete_policy(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

pub async fn update_approval_policy(
    path: web::Path<i32>,
    body: web::Json<crate::db::models_m1::UpdateApprovalPolicyRequest>,
    approval_service: web::Data<Arc<ApprovalService>>,
) -> AppResult<HttpResponse> {
    let policy = approval_service
        .update_policy(path.into_inner(), body.into_inner())
        .await?;
    Ok(HttpResponse::Ok().json(policy))
}

// ===========================================================================
// F3.1 — Payroll Run · Employees CRUD (real implementation)
// ===========================================================================

pub async fn create_employee(
    body: web::Json<crate::db::models_m1::CreateEmployeeRequest>,
    employee_service: web::Data<Arc<EmployeeService>>,
) -> AppResult<HttpResponse> {
    let employee = employee_service.create(body.into_inner()).await?;
    Ok(HttpResponse::Created().json(employee))
}

pub async fn list_employees(
    query: web::Query<ListEmployeesQuery>,
    employee_service: web::Data<Arc<EmployeeService>>,
) -> AppResult<HttpResponse> {
    let employees = employee_service
        .list(query.active_only.unwrap_or(false))
        .await?;
    Ok(HttpResponse::Ok().json(employees))
}

pub async fn get_employee(
    path: web::Path<i32>,
    employee_service: web::Data<Arc<EmployeeService>>,
) -> AppResult<HttpResponse> {
    let employee = employee_service.get(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(employee))
}

pub async fn update_employee(
    path: web::Path<i32>,
    body: web::Json<crate::db::models_m1::UpdateEmployeeRequest>,
    employee_service: web::Data<Arc<EmployeeService>>,
) -> AppResult<HttpResponse> {
    let employee = employee_service
        .update(path.into_inner(), body.into_inner())
        .await?;
    Ok(HttpResponse::Ok().json(employee))
}

pub async fn delete_employee(
    path: web::Path<i32>,
    employee_service: web::Data<Arc<EmployeeService>>,
) -> AppResult<HttpResponse> {
    employee_service.soft_delete(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(json!({ "ok": true })))
}

#[derive(serde::Deserialize)]
pub struct ListEmployeesQuery {
    pub active_only: Option<bool>,
}

// ===========================================================================
// F3.1 — Payroll Run (real implementation)
// ===========================================================================

#[derive(serde::Deserialize)]
pub struct ListPayrollRunsQuery {
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

pub async fn create_payroll_run(
    body: web::Json<crate::db::models_m1::CreatePayrollRunRequest>,
    payroll_service: web::Data<Arc<PayrollService>>,
    user: AuthenticatedUser,
) -> AppResult<HttpResponse> {
    let resp = payroll_service
        .create_run(body.into_inner(), user.user_id)
        .await?;
    // 422 when validation_errors is non-empty so the frontend can branch
    // on status code instead of inspecting the body shape.
    if !resp.validation_errors.is_empty() {
        return Ok(HttpResponse::UnprocessableEntity().json(resp));
    }
    Ok(HttpResponse::Created().json(resp))
}

pub async fn list_payroll_runs(
    query: web::Query<ListPayrollRunsQuery>,
    payroll_service: web::Data<Arc<PayrollService>>,
) -> AppResult<HttpResponse> {
    let runs = payroll_service
        .list_runs(query.limit.unwrap_or(50), query.offset.unwrap_or(0))
        .await?;
    Ok(HttpResponse::Ok().json(runs))
}

pub async fn get_payroll_run(
    path: web::Path<i32>,
    payroll_service: web::Data<Arc<PayrollService>>,
) -> AppResult<HttpResponse> {
    let summary = payroll_service.get_run_summary(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(summary))
}

pub async fn execute_payroll_run(
    path: web::Path<i32>,
    payroll_service: web::Data<Arc<PayrollService>>,
    user: AuthenticatedUser,
) -> AppResult<HttpResponse> {
    let outcome = payroll_service
        .execute_run(path.into_inner(), user.user_id)
        .await?;
    Ok(HttpResponse::Ok().json(outcome))
}

pub async fn cancel_payroll_run(
    path: web::Path<i32>,
    payroll_service: web::Data<Arc<PayrollService>>,
) -> AppResult<HttpResponse> {
    let run = payroll_service.cancel_run(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(run))
}

pub async fn retry_payroll_item(
    path: web::Path<(i32, i32)>,
    payroll_service: web::Data<Arc<PayrollService>>,
) -> AppResult<HttpResponse> {
    let (run_id, item_id) = path.into_inner();
    let item = payroll_service.retry_item(run_id, item_id).await?;
    Ok(HttpResponse::Ok().json(item))
}

pub async fn payroll_run_report(
    path: web::Path<i32>,
    payroll_service: web::Data<Arc<PayrollService>>,
) -> AppResult<HttpResponse> {
    let report = payroll_service.run_report(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(report))
}
