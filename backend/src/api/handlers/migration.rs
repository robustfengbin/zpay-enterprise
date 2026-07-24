//! F4.1 — Orchard → Ironwood migration endpoints (PRD-F4 §4.4).
//!
//! All endpoints are admin-only, mirroring the Orchard transfer handlers:
//! a migration moves the whole shielded treasury, so nothing here is
//! reachable by the operator or auditor roles.

use std::sync::Arc;

use actix_web::{web, HttpResponse};
use serde::Deserialize;

use crate::api::middleware::AuthenticatedUser;
use crate::db::models_f4::{CreateMigrationRunRequest, RejectMigrationRunRequest};
use crate::error::{AppError, AppResult};
use crate::services::MigrationService;

fn require_admin(user: &AuthenticatedUser) -> AppResult<()> {
    if user.role != "admin" {
        return Err(AppError::Forbidden(
            "Only admin can manage migration runs".to_string(),
        ));
    }
    Ok(())
}

/// POST /migrations — create a migration plan for a wallet
pub async fn create_migration_run(
    service: web::Data<Arc<MigrationService>>,
    user: AuthenticatedUser,
    request: web::Json<CreateMigrationRunRequest>,
) -> AppResult<HttpResponse> {
    require_admin(&user)?;
    let summary = service.create_run(request.into_inner(), user.user_id).await?;
    Ok(HttpResponse::Ok().json(summary))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

/// GET /migrations — list runs
pub async fn list_migration_runs(
    service: web::Data<Arc<MigrationService>>,
    user: AuthenticatedUser,
    query: web::Query<ListQuery>,
) -> AppResult<HttpResponse> {
    require_admin(&user)?;
    let runs = service
        .list_runs(query.limit.unwrap_or(50), query.offset.unwrap_or(0))
        .await?;
    Ok(HttpResponse::Ok().json(runs))
}

/// GET /migrations/{id} — run + items progress
pub async fn get_migration_run(
    service: web::Data<Arc<MigrationService>>,
    user: AuthenticatedUser,
    path: web::Path<i32>,
) -> AppResult<HttpResponse> {
    require_admin(&user)?;
    let summary = service.get_run_summary(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(summary))
}

/// POST /migrations/{id}/execute — start (immediate) or arm the schedule
/// (private). May pivot to awaiting_approval per F2.1 policy.
pub async fn execute_migration_run(
    service: web::Data<Arc<MigrationService>>,
    user: AuthenticatedUser,
    path: web::Path<i32>,
) -> AppResult<HttpResponse> {
    require_admin(&user)?;
    let outcome = service.execute_run(path.into_inner(), user.user_id).await?;
    Ok(HttpResponse::Ok().json(outcome))
}

/// POST /migrations/{id}/approve — checker approval (maker ≠ checker is
/// enforced at the SQL layer). The approval covers the whole window.
pub async fn approve_migration_run(
    service: web::Data<Arc<MigrationService>>,
    user: AuthenticatedUser,
    path: web::Path<i32>,
) -> AppResult<HttpResponse> {
    require_admin(&user)?;
    let run = service.approve_run(path.into_inner(), user.user_id).await?;
    Ok(HttpResponse::Ok().json(run))
}

/// POST /migrations/{id}/reject — checker rejection with a written reason
pub async fn reject_migration_run(
    service: web::Data<Arc<MigrationService>>,
    user: AuthenticatedUser,
    path: web::Path<i32>,
    request: web::Json<RejectMigrationRunRequest>,
) -> AppResult<HttpResponse> {
    require_admin(&user)?;
    let run = service
        .reject_run(path.into_inner(), user.user_id, &request.reason)
        .await?;
    Ok(HttpResponse::Ok().json(run))
}

/// POST /migrations/{id}/cancel — the only way to stop remaining batches
pub async fn cancel_migration_run(
    service: web::Data<Arc<MigrationService>>,
    user: AuthenticatedUser,
    path: web::Path<i32>,
) -> AppResult<HttpResponse> {
    require_admin(&user)?;
    let run = service.cancel_run(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(run))
}

/// POST /migrations/{run_id}/items/{item_id}/retry — replay one failed batch
pub async fn retry_migration_item(
    service: web::Data<Arc<MigrationService>>,
    user: AuthenticatedUser,
    path: web::Path<(i32, i32)>,
) -> AppResult<HttpResponse> {
    require_admin(&user)?;
    let (run_id, item_id) = path.into_inner();
    service.retry_item(run_id, item_id).await?;
    let summary = service.get_run_summary(run_id).await?;
    Ok(HttpResponse::Ok().json(summary))
}

/// GET /wallets/{id}/migration-status — banner data source
pub async fn wallet_migration_status(
    service: web::Data<Arc<MigrationService>>,
    user: AuthenticatedUser,
    path: web::Path<i32>,
) -> AppResult<HttpResponse> {
    require_admin(&user)?;
    let status = service.migration_status(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(status))
}
