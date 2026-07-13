use actix_web::{
    http::header::{CacheControl, CacheDirective},
    web, HttpRequest, HttpResponse,
};
use serde::Deserialize;
use uuid::Uuid;

use super::{error::ApiError, session::required_session, AppState};
use crate::persistence::E2eSettlementFaultAction;

pub(super) fn configure(config: &mut web::ServiceConfig) {
    config
        .route(
            "/api/e2e/settlements/{match_id}",
            web::get().to(settlement_audit),
        )
        .route(
            "/api/e2e/settlement-fault",
            web::get().to(settlement_fault_snapshot),
        )
        .route(
            "/api/e2e/settlement-fault",
            web::post().to(apply_settlement_fault),
        );
}

async fn settlement_audit(
    request: HttpRequest,
    state: web::Data<AppState>,
    match_id: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let session = required_session(&request, &state).await?;
    let match_id = Uuid::parse_str(&match_id).map_err(|_| ApiError::request_malformed())?;
    let repository = state
        .e2e_repository()
        .ok_or_else(ApiError::service_unavailable)?;
    let snapshot = repository
        .e2e_settlement_audit(match_id, session.account_id)
        .await
        .map_err(|_| ApiError::service_unavailable())?;
    Ok(HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoStore]))
        .json(snapshot))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SettlementFaultRequest {
    action: E2eSettlementFaultAction,
}

async fn settlement_fault_snapshot(
    request: HttpRequest,
    state: web::Data<AppState>,
) -> Result<HttpResponse, ApiError> {
    required_session(&request, &state).await?;
    let repository = state
        .e2e_repository()
        .ok_or_else(ApiError::service_unavailable)?;
    Ok(no_store_json(repository.e2e_settlement_fault_snapshot()))
}

async fn apply_settlement_fault(
    request: HttpRequest,
    state: web::Data<AppState>,
    body: web::Json<SettlementFaultRequest>,
) -> Result<HttpResponse, ApiError> {
    required_session(&request, &state).await?;
    let repository = state
        .e2e_repository()
        .ok_or_else(ApiError::service_unavailable)?;
    Ok(no_store_json(
        repository.apply_e2e_settlement_fault(body.action),
    ))
}

fn no_store_json(value: impl serde::Serialize) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoStore]))
        .json(value)
}
