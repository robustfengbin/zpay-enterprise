//! M1 F1.1 — AuditorAuthMiddleware.
//!
//! Mirrors the user-side AuthMiddleware but verifies a kind=auditor JWT
//! issued by AuditorService.  Scopes `/api/v1/auditor/*` so a leaked
//! admin token can never be replayed against an audit endpoint and vice
//! versa (see PRD-F1.1 §2 + the 2026-05-16 design note on dual JWT kinds).

use actix_web::{
    body::EitherBody,
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    http::header::AUTHORIZATION,
    Error, HttpMessage, HttpResponse,
};
use futures::future::{ok, LocalBoxFuture, Ready};
use std::rc::Rc;
use std::sync::Arc;

use crate::services::auditor_service::AuditorClaims;
use crate::services::AuditorService;

pub struct AuditorAuthMiddleware {
    pub auditor_service: Arc<AuditorService>,
}

impl<S, B> Transform<S, ServiceRequest> for AuditorAuthMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Transform = AuditorAuthMiddlewareService<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(AuditorAuthMiddlewareService {
            service: Rc::new(service),
            auditor_service: self.auditor_service.clone(),
        })
    }
}

pub struct AuditorAuthMiddlewareService<S> {
    service: Rc<S>,
    auditor_service: Arc<AuditorService>,
}

impl<S, B> Service<ServiceRequest> for AuditorAuthMiddlewareService<S>
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Future = LocalBoxFuture<'static, Result<Self::Response, Self::Error>>;

    forward_ready!(service);

    fn call(&self, req: ServiceRequest) -> Self::Future {
        let service = self.service.clone();
        let auditor_service = self.auditor_service.clone();

        Box::pin(async move {
            let token = match req
                .headers()
                .get(AUTHORIZATION)
                .and_then(|h| h.to_str().ok())
                .and_then(|h| h.strip_prefix("Bearer "))
            {
                Some(t) => t.to_string(),
                None => {
                    let response = HttpResponse::Unauthorized().json(serde_json::json!({
                        "error": "Missing or malformed Authorization header"
                    }));
                    return Ok(req.into_response(response).map_into_right_body());
                }
            };

            match auditor_service.verify_token(&token) {
                Ok(claims) => {
                    req.extensions_mut().insert(claims);
                    let res = service.call(req).await?;
                    Ok(res.map_into_left_body())
                }
                Err(_) => {
                    let response = HttpResponse::Unauthorized()
                        .json(serde_json::json!({ "error": "Invalid or expired auditor token" }));
                    Ok(req.into_response(response).map_into_right_body())
                }
            }
        })
    }
}

/// FromRequest extractor that pulls the AuditorClaims placed by the
/// AuditorAuthMiddleware.  Mirrors AuthenticatedUser — fail closed if the
/// claims are missing so handlers can never run unauthenticated.
#[derive(Debug, Clone)]
pub struct AuthenticatedAuditor {
    pub auditor_id: i32,
    pub email: String,
}

impl actix_web::FromRequest for AuthenticatedAuditor {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(
        req: &actix_web::HttpRequest,
        _payload: &mut actix_web::dev::Payload,
    ) -> Self::Future {
        match req.extensions().get::<AuditorClaims>().cloned() {
            Some(c) => ok(AuthenticatedAuditor {
                auditor_id: c.sub,
                email: c.email,
            }),
            None => futures::future::ready(Err(actix_web::error::ErrorUnauthorized(
                "Missing auditor authentication context",
            ))),
        }
    }
}
