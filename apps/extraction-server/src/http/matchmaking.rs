use actix_web::{web, HttpRequest};
use serde::Serialize;
use time::format_description::well_known::Rfc3339;

use super::{error::ApiError, session::required_session, AppState};

pub(super) fn configure(config: &mut web::ServiceConfig) {
    config.service(
        web::resource("/api/matchmaking/queue")
            .route(web::post().to(enqueue))
            .route(web::delete().to(dequeue)),
    );
}

async fn enqueue(
    request: HttpRequest,
    state: web::Data<AppState>,
) -> Result<web::Json<QueueResponse>, ApiError> {
    if !state.matchmaking_enabled() {
        return Err(ApiError::service_unavailable());
    }
    let session = required_session(&request, &state).await?;
    let queued = state
        .matchmaking()
        .enqueue(session.account_id, state.utc_now());
    let enqueued_at = queued
        .enqueued_at
        .format(&Rfc3339)
        .map_err(|_| ApiError::service_unavailable())?;
    Ok(web::Json(QueueResponse {
        status: "queued",
        position: Some(queued.position),
        enqueued_at: Some(enqueued_at),
        removed: None,
    }))
}

async fn dequeue(
    request: HttpRequest,
    state: web::Data<AppState>,
) -> Result<web::Json<QueueResponse>, ApiError> {
    if !state.matchmaking_enabled() {
        return Err(ApiError::service_unavailable());
    }
    let session = required_session(&request, &state).await?;
    let removed = state.matchmaking().dequeue(session.account_id);
    Ok(web::Json(QueueResponse {
        status: "idle",
        position: None,
        enqueued_at: None,
        removed: Some(removed),
    }))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct QueueResponse {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    enqueued_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    removed: Option<bool>,
}
