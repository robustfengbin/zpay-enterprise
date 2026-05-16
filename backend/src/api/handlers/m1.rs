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
use chrono::{Duration, Utc};
use serde_json::json;

use crate::error::AppResult;
use crate::services::EmployeeService;

// ===========================================================================
// F1.1 — Viewing Key audit (Admin side)
// ===========================================================================

pub async fn export_viewing_key(
    _path: web::Path<i32>,
    _body: web::Json<crate::db::models_m1::ExportViewingKeyRequest>,
) -> HttpResponse {
    HttpResponse::NotImplemented().json(json!({
        "error": "F1.1 export_viewing_key — not yet implemented",
        "stub": true,
    }))
}

pub async fn list_viewing_key_exports(_path: web::Path<i32>) -> HttpResponse {
    HttpResponse::Ok().json(json!([]))
}

pub async fn download_viewing_key(_token: web::Path<String>) -> HttpResponse {
    HttpResponse::NotImplemented().json(json!({
        "error": "F1.1 download_viewing_key — not yet implemented",
        "stub": true,
    }))
}

pub async fn create_auditor(
    _body: web::Json<crate::db::models_m1::CreateAuditorRequest>,
) -> HttpResponse {
    HttpResponse::NotImplemented().json(json!({
        "error": "F1.1 create_auditor — not yet implemented",
        "stub": true,
    }))
}

pub async fn list_auditors() -> HttpResponse {
    HttpResponse::Ok().json(json!([]))
}

pub async fn deactivate_auditor(_path: web::Path<i32>) -> HttpResponse {
    HttpResponse::NotImplemented().json(json!({ "stub": true }))
}

pub async fn create_payment_disclosure(
    _path: web::Path<i32>,
    _body: web::Json<crate::db::models_m1::CreatePaymentDisclosureRequest>,
) -> HttpResponse {
    HttpResponse::NotImplemented().json(json!({
        "error": "F1.1 create_payment_disclosure — not yet implemented",
        "stub": true,
    }))
}

pub async fn get_payment_disclosure(_path: web::Path<i32>) -> HttpResponse {
    HttpResponse::NotFound().json(json!({ "error": "stub: not found" }))
}

pub async fn download_payment_disclosure(_path: web::Path<i32>) -> HttpResponse {
    HttpResponse::NotImplemented().json(json!({ "stub": true }))
}

pub async fn list_payment_disclosures(_path: web::Path<i32>) -> HttpResponse {
    HttpResponse::Ok().json(json!([]))
}

// ===========================================================================
// F1.1 — Auditor side (independent prefix /api/v1/auditor/*)
// ===========================================================================

pub async fn auditor_login(
    _body: web::Json<crate::db::models_m1::AuditorLoginRequest>,
) -> HttpResponse {
    HttpResponse::NotImplemented().json(json!({
        "error": "F1.1 auditor_login — not yet implemented",
        "stub": true,
    }))
}

pub async fn auditor_me() -> HttpResponse {
    HttpResponse::Unauthorized().json(json!({ "error": "stub: not logged in" }))
}

pub async fn auditor_list_wallets() -> HttpResponse {
    HttpResponse::Ok().json(json!([]))
}

pub async fn auditor_wallet_balance(_path: web::Path<i32>) -> HttpResponse {
    HttpResponse::Ok().json(json!({
        "native_balance": "0",
        "tokens": [],
        "stub": true,
    }))
}

pub async fn auditor_wallet_transfers(_path: web::Path<i32>) -> HttpResponse {
    HttpResponse::Ok().json(json!([]))
}

pub async fn auditor_wallet_disclosures(_path: web::Path<i32>) -> HttpResponse {
    HttpResponse::Ok().json(json!([]))
}

// ===========================================================================
// F2.1 — Maker / Checker dual-sign
// ===========================================================================

pub async fn approve_transfer(
    _path: web::Path<i32>,
    _body: web::Json<crate::db::models_m1::ApprovalDecisionRequest>,
) -> HttpResponse {
    HttpResponse::NotImplemented().json(json!({
        "error": "F2.1 approve_transfer — not yet implemented",
        "stub": true,
    }))
}

pub async fn reject_transfer(
    _path: web::Path<i32>,
    _body: web::Json<crate::db::models_m1::ApprovalDecisionRequest>,
) -> HttpResponse {
    HttpResponse::NotImplemented().json(json!({
        "error": "F2.1 reject_transfer — not yet implemented",
        "stub": true,
    }))
}

pub async fn list_approvals_for_transfer(_path: web::Path<i32>) -> HttpResponse {
    HttpResponse::Ok().json(json!([]))
}

pub async fn list_pending_approvals() -> HttpResponse {
    HttpResponse::Ok().json(json!({
        "items": [],
        "stub": true,
    }))
}

pub async fn list_approval_policies() -> HttpResponse {
    HttpResponse::Ok().json(json!([]))
}

pub async fn create_approval_policy(
    _body: web::Json<crate::db::models_m1::CreateApprovalPolicyRequest>,
) -> HttpResponse {
    HttpResponse::NotImplemented().json(json!({
        "error": "F2.1 create_approval_policy — not yet implemented",
        "stub": true,
    }))
}

pub async fn delete_approval_policy(_path: web::Path<i32>) -> HttpResponse {
    HttpResponse::NotImplemented().json(json!({ "stub": true }))
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

pub async fn create_payroll_run(
    _body: web::Json<crate::db::models_m1::CreatePayrollRunRequest>,
) -> HttpResponse {
    let now = Utc::now();
    HttpResponse::Ok().json(json!({
        "run_id": 0,
        "item_count": 0,
        "validation_errors": [],
        "stub": true,
        "_created_at_marker": now.to_rfc3339(),
        "_expiry_marker": (now + Duration::hours(24)).to_rfc3339(),
    }))
}

pub async fn list_payroll_runs() -> HttpResponse {
    HttpResponse::Ok().json(json!([]))
}

pub async fn get_payroll_run(_path: web::Path<i32>) -> HttpResponse {
    HttpResponse::NotFound().json(json!({ "error": "stub: not found" }))
}

pub async fn execute_payroll_run(_path: web::Path<i32>) -> HttpResponse {
    HttpResponse::NotImplemented().json(json!({
        "error": "F3.1 execute_payroll_run — not yet implemented",
        "stub": true,
    }))
}

pub async fn cancel_payroll_run(_path: web::Path<i32>) -> HttpResponse {
    HttpResponse::NotImplemented().json(json!({ "stub": true }))
}

pub async fn retry_payroll_item(_path: web::Path<(i32, i32)>) -> HttpResponse {
    HttpResponse::NotImplemented().json(json!({ "stub": true }))
}

pub async fn payroll_run_report(_path: web::Path<i32>) -> HttpResponse {
    HttpResponse::NotImplemented().json(json!({ "stub": true }))
}
