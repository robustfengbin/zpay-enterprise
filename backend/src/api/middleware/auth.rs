use actix_web::{
    body::EitherBody,
    dev::{forward_ready, Service, ServiceRequest, ServiceResponse, Transform},
    http::header::AUTHORIZATION,
    Error, HttpMessage, HttpResponse,
};
use futures::future::{ok, LocalBoxFuture, Ready};
use std::rc::Rc;
use std::sync::Arc;

use crate::services::AuthService;

pub struct AuthMiddleware {
    pub auth_service: Arc<AuthService>,
}

impl<S, B> Transform<S, ServiceRequest> for AuthMiddleware
where
    S: Service<ServiceRequest, Response = ServiceResponse<B>, Error = Error> + 'static,
    S::Future: 'static,
    B: 'static,
{
    type Response = ServiceResponse<EitherBody<B>>;
    type Error = Error;
    type Transform = AuthMiddlewareService<S>;
    type InitError = ();
    type Future = Ready<Result<Self::Transform, Self::InitError>>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(AuthMiddlewareService {
            service: Rc::new(service),
            auth_service: self.auth_service.clone(),
        })
    }
}

pub struct AuthMiddlewareService<S> {
    service: Rc<S>,
    auth_service: Arc<AuthService>,
}

impl<S, B> Service<ServiceRequest> for AuthMiddlewareService<S>
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
        let auth_service = self.auth_service.clone();

        Box::pin(async move {
            // Extract token from Authorization header
            let auth_header = req
                .headers()
                .get(AUTHORIZATION)
                .and_then(|h| h.to_str().ok());

            let token = match auth_header {
                Some(header) if header.starts_with("Bearer ") => &header[7..],
                _ => {
                    let response = HttpResponse::Unauthorized()
                        .json(serde_json::json!({"error": "Missing or invalid authorization header"}));
                    return Ok(req.into_response(response).map_into_right_body());
                }
            };

            // Verify token
            match auth_service.verify_token(token) {
                Ok(claims) => {
                    // Store claims in request extensions
                    req.extensions_mut().insert(claims);
                    let res = service.call(req).await?;
                    Ok(res.map_into_left_body())
                }
                Err(_) => {
                    let response = HttpResponse::Unauthorized()
                        .json(serde_json::json!({"error": "Invalid or expired token"}));
                    Ok(req.into_response(response).map_into_right_body())
                }
            }
        })
    }
}

/// Extractor for authenticated user claims
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct AuthenticatedUser {
    pub user_id: i32,
    pub username: String,
    pub role: String,
}

impl actix_web::FromRequest for AuthenticatedUser {
    type Error = Error;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(
        req: &actix_web::HttpRequest,
        _payload: &mut actix_web::dev::Payload,
    ) -> Self::Future {
        let claims = req.extensions().get::<crate::services::auth_service::Claims>().cloned();

        // Fail closed: if the AuthMiddleware did not run (e.g. someone added
        // a new handler outside the middleware-wrapped scope), refuse the
        // request rather than silently exposing a synthetic empty user.
        // See SECURITY.md and the 2026-04-25 audit (P1-2) for rationale.
        match claims {
            Some(c) => ok(AuthenticatedUser {
                user_id: c.sub,
                username: c.username,
                role: c.role,
            }),
            None => futures::future::ready(Err(actix_web::error::ErrorUnauthorized(
                "Missing authentication context",
            ))),
        }
    }
}
