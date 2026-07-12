use actix_web::{
    http::header::{CacheControl, CacheDirective},
    web, HttpRequest, HttpResponse,
};
use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;

use crate::auth::{removal_cookie, session_cookie, SessionView};

use super::{
    error::ApiError,
    session::{optional_session, session_token},
    AppState,
};

pub(super) fn configure(config: &mut web::ServiceConfig) {
    config.service(
        web::scope("/api/auth")
            .route("/siwe/nonce", web::get().to(nonce))
            .route("/siwe/verify", web::post().to(verify))
            .route("/session", web::get().to(session))
            .route("/logout", web::post().to(logout)),
    );
}

async fn nonce(request: HttpRequest, state: web::Data<AppState>) -> Result<HttpResponse, ApiError> {
    if !state.auth_login_enabled() {
        return Err(ApiError::service_unavailable());
    }
    if !state.allow_nonce_request(request.peer_addr()) {
        return Err(ApiError::service_unavailable());
    }
    let auth = state.auth().ok_or_else(ApiError::service_unavailable)?;
    let issued = auth.issue_nonce().await.map_err(ApiError::from)?;
    let expires_at = issued
        .expires_at
        .format(&Rfc3339)
        .map_err(|_| ApiError::service_unavailable())?;
    Ok(HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoStore]))
        .json(NonceResponse {
            nonce: issued.nonce,
            expires_at,
        }))
}

#[cfg(feature = "engine")]
async fn verify(
    http_request: HttpRequest,
    state: web::Data<AppState>,
    payload: web::Json<VerifyRequest>,
    server: Option<web::Data<actix::Addr<voxelize::Server>>>,
) -> Result<HttpResponse, ApiError> {
    let created = create_session(&state, &payload, http_request.peer_addr()).await?;
    if let Some(server) = server {
        for session_id in &created.revoked_session_ids {
            let _ = server
                .send(voxelize::CloseAuthenticatedSession {
                    session_id: session_id.to_string(),
                })
                .await;
        }
    }
    let config = state
        .auth()
        .ok_or_else(ApiError::service_unavailable)?
        .config();
    Ok(session_response(config, created.token))
}

#[cfg(not(feature = "engine"))]
async fn verify(
    http_request: HttpRequest,
    state: web::Data<AppState>,
    payload: web::Json<VerifyRequest>,
) -> Result<HttpResponse, ApiError> {
    let created = create_session(&state, &payload, http_request.peer_addr()).await?;
    let config = state
        .auth()
        .ok_or_else(ApiError::service_unavailable)?
        .config();
    Ok(session_response(config, created.token))
}

async fn create_session(
    state: &web::Data<AppState>,
    request: &VerifyRequest,
    peer_addr: Option<std::net::SocketAddr>,
) -> Result<crate::auth::CreatedSession, ApiError> {
    if !state.auth_login_enabled() {
        return Err(ApiError::service_unavailable());
    }
    if !state.allow_verification_request(peer_addr) {
        return Err(ApiError::service_unavailable());
    }
    let auth = state.auth().ok_or_else(ApiError::service_unavailable)?;
    auth.verify_and_create_session(&request.message, &request.signature)
        .await
        .map_err(ApiError::from)
}

fn session_response(config: &crate::auth::AuthConfig, token: String) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoStore]))
        .cookie(session_cookie(config, token))
        .json(true)
}

async fn session(
    request: HttpRequest,
    state: web::Data<AppState>,
) -> Result<HttpResponse, ApiError> {
    let session = optional_session(&request, &state).await?;
    Ok(HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoStore]))
        .json(session.as_ref().map(SessionView::from)))
}

#[cfg(feature = "engine")]
async fn logout(
    request: HttpRequest,
    state: web::Data<AppState>,
    server: Option<web::Data<actix::Addr<voxelize::Server>>>,
) -> Result<HttpResponse, ApiError> {
    let auth = state.auth().ok_or_else(ApiError::service_unavailable)?;
    let token = session_token(&request, &state);
    let session_id = auth
        .revoke_token(token.as_deref())
        .await
        .map_err(ApiError::from)?;
    if let (Some(session_id), Some(server)) = (session_id, server) {
        let _ = server
            .send(voxelize::CloseAuthenticatedSession {
                session_id: session_id.to_string(),
            })
            .await;
    }
    Ok(logout_response(auth.config()))
}

#[cfg(not(feature = "engine"))]
async fn logout(
    request: HttpRequest,
    state: web::Data<AppState>,
) -> Result<HttpResponse, ApiError> {
    let auth = state.auth().ok_or_else(ApiError::service_unavailable)?;
    let token = session_token(&request, &state);
    auth.revoke_token(token.as_deref())
        .await
        .map_err(ApiError::from)?;
    Ok(logout_response(auth.config()))
}

fn logout_response(config: &crate::auth::AuthConfig) -> HttpResponse {
    HttpResponse::NoContent()
        .insert_header(CacheControl(vec![CacheDirective::NoStore]))
        .cookie(removal_cookie(config))
        .finish()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct NonceResponse {
    nonce: String,
    expires_at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifyRequest {
    message: String,
    signature: String,
}
