//! M1 2026-06 data models — F1.1 (Auditor / ViewingKey / Disclosure) +
//! F2.1 (Maker/Checker) + F3.1 (Payroll Run).
//!
//! All structs are additive — existing `models.rs` is not modified except
//! for adding 4 new `TransferStatus` enum variants (additive, default value
//! unchanged), which is backward-compatible per Robust's 2026-05-16 rule.

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ---------------------------------------------------------------------------
// F1.1 — Viewing Key audit layer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Auditor {
    pub id: i32,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub name: String,
    pub invited_by_user_id: i32,
    pub active: bool,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditorResponse {
    pub id: i32,
    pub email: String,
    pub name: String,
    pub active: bool,
    pub last_login_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

impl From<Auditor> for AuditorResponse {
    fn from(a: Auditor) -> Self {
        AuditorResponse {
            id: a.id,
            email: a.email,
            name: a.name,
            active: a.active,
            last_login_at: a.last_login_at,
            created_at: a.created_at,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AuditorWalletScope {
    pub id: i32,
    pub auditor_id: i32,
    pub wallet_id: i32,
    pub granted_by_user_id: i32,
    pub scope_start_ts: DateTime<Utc>,
    pub scope_end_ts: DateTime<Utc>,
    pub max_disclosure_count: i32,
    pub current_count: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ViewingKeyExport {
    pub id: i32,
    pub wallet_id: i32,
    pub exported_by_user_id: i32,
    pub key_type: String,
    #[serde(skip_serializing)]
    pub encrypted_payload: Vec<u8>,
    pub payload_hash: String,
    #[serde(skip_serializing)]
    pub download_token: String,
    pub downloaded_at: Option<DateTime<Utc>>,
    pub downloaded_by_ip: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ViewingKeyExportResponse {
    pub id: i32,
    pub wallet_id: i32,
    pub key_type: String,
    pub downloaded_at: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PaymentDisclosure {
    pub id: i32,
    pub wallet_id: i32,
    pub generated_by_user_id: i32,
    pub granularity: String,
    pub scope_param: serde_json::Value,
    pub tx_count: i32,
    pub disclosure_json: Option<serde_json::Value>,
    pub format: String,
    pub file_path: Option<String>,
    pub status: String,
    pub error_message: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExportViewingKeyRequest {
    pub key_type: String, // ovk | ivk | ufvk
    pub password: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExportViewingKeyResponse {
    pub export_id: i32,
    pub download_token: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateAuditorRequest {
    pub email: String,
    pub name: String,
    pub wallet_ids: Vec<i32>,
    pub scope_start: DateTime<Utc>,
    pub scope_end: DateTime<Utc>,
    #[serde(default = "default_max_count")]
    pub max_count: i32,
}

fn default_max_count() -> i32 {
    10
}

#[derive(Debug, Clone, Serialize)]
pub struct CreateAuditorResponse {
    pub auditor_id: i32,
    pub invitation_link: String,
    pub temp_password: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatePaymentDisclosureRequest {
    pub granularity: String, // tx | address | range
    pub scope_param: serde_json::Value,
    pub format: String, // pdf | csv | json
}

#[derive(Debug, Clone, Serialize)]
pub struct PaymentDisclosureResponse {
    pub disclosure_id: i32,
    pub status: String,
    pub tx_count: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuditorLoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuditorLoginResponse {
    pub token: String,
    pub auditor: AuditorResponse,
}

// ---------------------------------------------------------------------------
// F2.1 — Maker / Checker dual-sign
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct TransferApproval {
    pub id: i32,
    pub transfer_id: i32,
    pub approver_user_id: i32,
    pub decision: String,
    pub reason: Option<String>,
    pub policy_snapshot: Option<serde_json::Value>,
    pub idempotency_key: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ApprovalPolicy {
    pub id: i32,
    pub scope: String,
    pub scope_id: Option<i32>,
    pub chain: String,
    pub token: String,
    pub amount_threshold: Decimal,
    pub sla_minutes: i32,
    pub required_count: i32,
    pub enabled: bool,
    pub created_by: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// PATCH-style update.  `scope` / `chain` / `token` / `created_by` are
/// intentionally immutable — changing them would invalidate every prior
/// approval decision that referenced this policy snapshot.  Operators who
/// need a different scope/chain/token should create a new policy and
/// disable the old one.
#[derive(Debug, Clone, Deserialize)]
pub struct UpdateApprovalPolicyRequest {
    pub amount_threshold: Option<String>,
    pub sla_minutes: Option<i32>,
    pub required_count: Option<i32>,
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateApprovalPolicyRequest {
    pub scope: String,
    pub scope_id: Option<i32>,
    pub chain: String,
    pub token: String,
    pub amount_threshold: String,
    #[serde(default = "default_sla")]
    pub sla_minutes: i32,
    #[serde(default = "default_required_count")]
    pub required_count: i32,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_sla() -> i32 {
    1440
}

fn default_required_count() -> i32 {
    1
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApprovalDecisionRequest {
    pub decision: String, // approve | reject
    pub reason: Option<String>,
    pub idempotency_key: Option<String>,
}

// ---------------------------------------------------------------------------
// F3.1 — batch Payroll Run
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Employee {
    pub id: i32,
    pub employee_code: String,
    pub name: String,
    pub wallet_address: String,
    pub chain: String,
    pub tags: Option<serde_json::Value>,
    pub active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateEmployeeRequest {
    pub employee_code: String,
    pub name: String,
    pub wallet_address: String,
    #[serde(default = "default_chain_zcash")]
    pub chain: String,
    pub tags: Option<serde_json::Value>,
}

fn default_chain_zcash() -> String {
    "zcash".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateEmployeeRequest {
    pub name: Option<String>,
    pub wallet_address: Option<String>,
    pub tags: Option<serde_json::Value>,
    pub active: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PayrollRun {
    pub id: i32,
    pub pay_period: String,
    pub source_wallet_id: i32,
    pub total_amount: Decimal,
    pub item_count: i32,
    pub status: String,
    pub created_by_user_id: i32,
    pub executed_by_user_id: Option<i32>,
    pub executed_at: Option<DateTime<Utc>>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct PayrollItem {
    pub id: i32,
    pub run_id: i32,
    pub employee_id: Option<i32>,
    pub employee_address: String,
    pub amount: Decimal,
    pub memo: Option<String>,
    pub status: String,
    pub tx_hash: Option<String>,
    pub block_number: Option<i64>,
    pub transfer_id: Option<i32>,
    pub error_message: Option<String>,
    pub retry_count: i32,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreatePayrollRunRequest {
    pub pay_period: String,
    pub source_wallet_id: i32,
    pub items: Vec<PayrollItemInput>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PayrollItemInput {
    pub employee_code: Option<String>,
    pub employee_address: String,
    pub amount: String,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PayrollRunSummary {
    pub run: PayrollRun,
    pub items: Vec<PayrollItem>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CreatePayrollRunResponse {
    pub run_id: i32,
    pub item_count: i32,
    pub validation_errors: Vec<ValidationError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ValidationError {
    pub row_index: usize,
    pub field: String,
    pub message: String,
}
