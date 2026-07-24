//! F4 2026-07 — models for the batch privacy transfer & Ironwood migration
//! module (PRD-F4, docs/2026-07-24/).

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ---------------------------------------------------------------------------
// F4.1 — migration runs
// ---------------------------------------------------------------------------

/// Run status lifecycle:
/// `pending → awaiting_approval → approved → executing →
///  completed | partial | failed`, with `rejected` / `canceled` as
/// terminal side exits. An approval covers the run's whole scheduling
/// window: batches execute unattended once the run is `executing`, and
/// cancel is the only way to stop remaining batches (PRD-F4 §4.2 F4.1.5).
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MigrationRun {
    pub id: i32,
    pub source_wallet_id: i32,
    /// `immediate` | `private`
    pub mode: String,
    pub batch_count: i32,
    pub window_hours: i32,
    pub total_amount: Decimal,
    pub item_count: i32,
    pub status: String,
    pub created_by_user_id: i32,
    pub approved_by_user_id: Option<i32>,
    pub reject_reason: Option<String>,
    pub executed_by_user_id: Option<i32>,
    pub executed_at: Option<DateTime<Utc>>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Item status lifecycle: `pending → submitted | failed | canceled`.
/// `failed` items may be retried (back to a fresh attempt) without
/// blocking sibling items.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct MigrationItem {
    pub id: i32,
    pub run_id: i32,
    pub seq: i32,
    pub amount: Decimal,
    /// NULL = eligible immediately once the run is executing
    pub scheduled_at: Option<DateTime<Utc>>,
    pub status: String,
    pub tx_hash: Option<String>,
    pub error_message: Option<String>,
    pub retry_count: i32,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CreateMigrationRunRequest {
    pub source_wallet_id: i32,
    /// `immediate` | `private`
    pub mode: String,
    /// private mode only; defaults applied by the service
    pub batch_count: Option<u32>,
    /// private mode only; defaults applied by the service
    pub window_hours: Option<u32>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MigrationRunSummary {
    pub run: MigrationRun,
    pub items: Vec<MigrationItem>,
}

/// Banner data for the wallet page: does this wallet still hold funds in
/// the legacy Orchard pool that should move through the turnstile?
#[derive(Debug, Clone, Serialize)]
pub struct MigrationStatusResponse {
    pub wallet_id: i32,
    pub spendable_zatoshis: u64,
    pub total_zatoshis: u64,
    pub unspent_note_count: u32,
    /// Non-terminal migration run for this wallet, if one exists
    pub active_run_id: Option<i32>,
    pub active_run_status: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RejectMigrationRunRequest {
    pub reason: String,
}

// ---------------------------------------------------------------------------
// F4.2 — generic batch privacy transfer (schema finalized in F4 migrations;
// models land with the execution layer after F4.0 merges — kept here so the
// table shape and the Rust shape are reviewed together)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BatchTransferRun {
    pub id: i32,
    pub title: String,
    pub source_wallet_id: i32,
    /// `off` | `staggered`
    pub privacy_mode: String,
    pub batch_count: i32,
    pub window_hours: i32,
    pub total_amount: Decimal,
    pub item_count: i32,
    pub status: String,
    pub created_by_user_id: i32,
    pub approved_by_user_id: Option<i32>,
    pub executed_by_user_id: Option<i32>,
    pub executed_at: Option<DateTime<Utc>>,
    pub notes: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct BatchTransferItem {
    pub id: i32,
    pub run_id: i32,
    pub seq: i32,
    pub recipient_address: String,
    pub amount: Decimal,
    pub memo: Option<String>,
    pub scheduled_at: Option<DateTime<Utc>>,
    pub status: String,
    pub tx_hash: Option<String>,
    pub error_message: Option<String>,
    pub retry_count: i32,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
