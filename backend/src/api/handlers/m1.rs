//! M1 2026-06 HTTP handlers — stub layer for F1.1 / F2.1 / F3.1.
//!
//! Each handler returns a well-typed empty / mock response so the frontend
//! can wire up axios clients and verify the contract.  Business logic is
//! filled in incrementally — see the per-feature PRD §5 + §7 milestones.
//!
//! All handlers are additive — existing /auth /wallets /transfers handlers
//! in this directory are NOT modified (per Robust's 2026-05-16 rule).

use std::sync::Arc;

use actix_web::{web, HttpRequest, HttpResponse};
use serde_json::json;

use crate::api::middleware::{AuthenticatedAuditor, AuthenticatedUser};
use crate::db::repositories::{PaymentDisclosureRepository, TransferRepository, WalletRepository};
use crate::error::{AppError, AppResult};
use crate::services::{
    ApprovalService, AuditorService, EmployeeService, PayrollService, PaymentDisclosureService,
    TransferService, ViewingKeyService, WalletService,
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
// F1.1 — Auditor side (independent prefix /api/v1/auditor/*).  All routes
// except /login sit behind AuditorAuthMiddleware which verifies the
// kind=auditor JWT and exposes AuthenticatedAuditor as a FromRequest
// extractor — handlers below stay free of inline token verification.
// ===========================================================================

pub async fn auditor_login(
    body: web::Json<crate::db::models_m1::AuditorLoginRequest>,
    auditor_service: web::Data<Arc<AuditorService>>,
) -> AppResult<HttpResponse> {
    let resp = auditor_service.login(body.into_inner()).await?;
    Ok(HttpResponse::Ok().json(resp))
}

pub async fn auditor_me(
    auditor: AuthenticatedAuditor,
    auditor_service: web::Data<Arc<AuditorService>>,
) -> AppResult<HttpResponse> {
    let row = auditor_service
        .find_by_id(auditor.auditor_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("auditor {} not found", auditor.auditor_id)))?;
    let scopes = auditor_service.list_scopes(auditor.auditor_id).await?;
    Ok(HttpResponse::Ok().json(json!({
        "auditor": crate::db::models_m1::AuditorResponse::from(row),
        "scopes": scopes,
    })))
}

pub async fn auditor_list_wallets(
    auditor: AuthenticatedAuditor,
    auditor_service: web::Data<Arc<AuditorService>>,
    wallet_repo: web::Data<WalletRepository>,
    transfer_repo: web::Data<TransferRepository>,
    disclosure_repo: web::Data<PaymentDisclosureRepository>,
) -> AppResult<HttpResponse> {
    let scopes = auditor_service.list_scopes(auditor.auditor_id).await?;

    // Build the auditor dashboard summary row per wallet in scope.
    // Aggregates (tx count / last activity / pending disclosures) come from
    // single-shot queries — small N (auditors typically have 1–5 wallets in
    // scope) so a per-row round trip is acceptable.  Wallet name + address
    // come from the wallets table, which is the authoritative source.
    let mut wallets = Vec::with_capacity(scopes.len());
    for scope in scopes {
        let Some(w) = wallet_repo.find_by_id(scope.wallet_id).await? else {
            continue;
        };
        let (total_tx_count, last_activity_at) = transfer_repo
            .aggregate_in_window(w.id, scope.scope_start_ts, scope.scope_end_ts)
            .await?;
        let pending_disclosures = disclosure_repo
            .count_generating_for_wallet(w.id)
            .await?;
        wallets.push(json!({
            "wallet_id": w.id,
            "wallet_name": w.name,
            "address": w.address,
            "chain": w.chain,
            "scope_start": scope.scope_start_ts,
            "scope_end": scope.scope_end_ts,
            "max_disclosure_count": scope.max_disclosure_count,
            "current_count": scope.current_count,
            "total_tx_count": total_tx_count,
            "last_activity_at": last_activity_at,
            "pending_disclosures": pending_disclosures,
        }));
    }
    Ok(HttpResponse::Ok().json(wallets))
}

pub async fn auditor_wallet_balance(
    auditor: AuthenticatedAuditor,
    path: web::Path<i32>,
    auditor_service: web::Data<Arc<AuditorService>>,
    wallet_service: web::Data<Arc<WalletService>>,
    wallet_repo: web::Data<WalletRepository>,
) -> AppResult<HttpResponse> {
    let wallet_id = path.into_inner();
    auditor_service
        .assert_wallet_in_scope(auditor.auditor_id, wallet_id)
        .await?;

    let wallet = wallet_repo
        .find_by_id(wallet_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("wallet {} not found", wallet_id)))?;

    // For Zcash wallets surface both transparent and shielded balance via
    // the combined helper (it falls back gracefully when Orchard isn't
    // enabled).  For other chains fall back to the canonical balance call.
    if wallet.chain == "zcash" {
        let combined = wallet_service.get_combined_zcash_balance(wallet_id).await?;
        Ok(HttpResponse::Ok().json(combined))
    } else {
        let balance = wallet_service.get_balance(&wallet.address, &wallet.chain).await?;
        Ok(HttpResponse::Ok().json(balance))
    }
}

#[derive(serde::Deserialize)]
pub struct AuditorTransfersQuery {
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

pub async fn auditor_wallet_transfers(
    auditor: AuthenticatedAuditor,
    path: web::Path<i32>,
    query: web::Query<AuditorTransfersQuery>,
    auditor_service: web::Data<Arc<AuditorService>>,
    transfer_service: web::Data<Arc<TransferService>>,
) -> AppResult<HttpResponse> {
    let wallet_id = path.into_inner();
    // Scope check returns the row so we can use its time window directly —
    // no need to re-query, and we are guaranteed the window matches the
    // identity that was just verified by AuditorAuthMiddleware.
    let scope = auditor_service
        .assert_wallet_in_scope(auditor.auditor_id, wallet_id)
        .await?;

    let limit = query.limit.unwrap_or(50).clamp(1, 200);
    let offset = query.offset.unwrap_or(0).max(0);

    let transfers = transfer_service
        .list_wallet_transfers_in_window(
            wallet_id,
            scope.scope_start_ts,
            scope.scope_end_ts,
            limit,
            offset,
        )
        .await?;

    Ok(HttpResponse::Ok().json(json!({
        "wallet_id": wallet_id,
        "scope_start": scope.scope_start_ts,
        "scope_end": scope.scope_end_ts,
        "transfers": transfers,
    })))
}

pub async fn auditor_wallet_disclosures(
    auditor: AuthenticatedAuditor,
    path: web::Path<i32>,
    auditor_service: web::Data<Arc<AuditorService>>,
) -> AppResult<HttpResponse> {
    auditor_service
        .assert_wallet_in_scope(auditor.auditor_id, path.into_inner())
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
