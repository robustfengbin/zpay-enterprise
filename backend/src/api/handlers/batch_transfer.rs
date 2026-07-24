//! F4.2 — batch privacy transfer endpoints (PRD-F4 §5). Mirrors the
//! migration endpoints one-for-one; admin-only for the same reason —
//! these move treasury funds.

use std::sync::Arc;

use actix_web::{web, HttpResponse};
use serde::Deserialize;

use crate::api::middleware::AuthenticatedUser;
use crate::db::models_f4::{CreateBatchTransferRunRequest, RejectBatchTransferRunRequest};
use crate::error::{AppError, AppResult};
use crate::services::BatchTransferService;

fn require_admin(user: &AuthenticatedUser) -> AppResult<()> {
    if user.role != "admin" {
        return Err(AppError::Forbidden(
            "Only admin can manage batch transfer runs".to_string(),
        ));
    }
    Ok(())
}

/// POST /batch-transfers — validate the imported rows and create a plan
pub async fn create_batch_transfer_run(
    service: web::Data<Arc<BatchTransferService>>,
    user: AuthenticatedUser,
    request: web::Json<CreateBatchTransferRunRequest>,
) -> AppResult<HttpResponse> {
    require_admin(&user)?;
    let response = service.create_run(request.into_inner(), user.user_id).await?;
    Ok(HttpResponse::Ok().json(response))
}

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

/// GET /batch-transfers — list runs
pub async fn list_batch_transfer_runs(
    service: web::Data<Arc<BatchTransferService>>,
    user: AuthenticatedUser,
    query: web::Query<ListQuery>,
) -> AppResult<HttpResponse> {
    require_admin(&user)?;
    let runs = service
        .list_runs(query.limit.unwrap_or(50), query.offset.unwrap_or(0))
        .await?;
    Ok(HttpResponse::Ok().json(runs))
}

/// GET /batch-transfers/{id} — run + items progress
pub async fn get_batch_transfer_run(
    service: web::Data<Arc<BatchTransferService>>,
    user: AuthenticatedUser,
    path: web::Path<i32>,
) -> AppResult<HttpResponse> {
    require_admin(&user)?;
    let summary = service.get_run_summary(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(summary))
}

/// POST /batch-transfers/{id}/execute — start now or arm the schedule.
/// May pivot to awaiting_approval per F2.1 policy.
pub async fn execute_batch_transfer_run(
    service: web::Data<Arc<BatchTransferService>>,
    user: AuthenticatedUser,
    path: web::Path<i32>,
) -> AppResult<HttpResponse> {
    require_admin(&user)?;
    let outcome = service.execute_run(path.into_inner(), user.user_id).await?;
    Ok(HttpResponse::Ok().json(outcome))
}

/// POST /batch-transfers/{id}/approve — checker approval (maker ≠ checker
/// enforced at the SQL layer). The approval covers the whole window.
pub async fn approve_batch_transfer_run(
    service: web::Data<Arc<BatchTransferService>>,
    user: AuthenticatedUser,
    path: web::Path<i32>,
) -> AppResult<HttpResponse> {
    require_admin(&user)?;
    let run = service.approve_run(path.into_inner(), user.user_id).await?;
    Ok(HttpResponse::Ok().json(run))
}

/// POST /batch-transfers/{id}/reject — checker rejection with a reason
pub async fn reject_batch_transfer_run(
    service: web::Data<Arc<BatchTransferService>>,
    user: AuthenticatedUser,
    path: web::Path<i32>,
    request: web::Json<RejectBatchTransferRunRequest>,
) -> AppResult<HttpResponse> {
    require_admin(&user)?;
    let run = service
        .reject_run(path.into_inner(), user.user_id, &request.reason)
        .await?;
    Ok(HttpResponse::Ok().json(run))
}

/// POST /batch-transfers/{id}/cancel — the only way to stop remaining items
pub async fn cancel_batch_transfer_run(
    service: web::Data<Arc<BatchTransferService>>,
    user: AuthenticatedUser,
    path: web::Path<i32>,
) -> AppResult<HttpResponse> {
    require_admin(&user)?;
    let run = service.cancel_run(path.into_inner()).await?;
    Ok(HttpResponse::Ok().json(run))
}

/// POST /batch-transfers/{run_id}/items/{item_id}/retry — replay one failed item
pub async fn retry_batch_transfer_item(
    service: web::Data<Arc<BatchTransferService>>,
    user: AuthenticatedUser,
    path: web::Path<(i32, i32)>,
) -> AppResult<HttpResponse> {
    require_admin(&user)?;
    let (run_id, item_id) = path.into_inner();
    service.retry_item(run_id, item_id).await?;
    let summary = service.get_run_summary(run_id).await?;
    Ok(HttpResponse::Ok().json(summary))
}
