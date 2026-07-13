#[cfg(feature = "engine")]
use std::sync::Arc;

use actix_web::{
    http::header::{CacheControl, CacheDirective},
    web, HttpRequest, HttpResponse,
};
use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

#[cfg(feature = "engine")]
use actix::Addr;
#[cfg(feature = "engine")]
use voxelize::Server;

use super::{error::ApiError, session::required_session, AppState};
#[cfg(feature = "engine")]
use crate::engine_matchmaking::EngineMatchWorldRuntime;
use crate::matchmaking::{QueueSnapshot, QueueStatus};

pub(super) fn configure(config: &mut web::ServiceConfig) {
    config.service(
        web::resource("/api/matchmaking/queue")
            .route(web::get().to(queue_snapshot))
            .route(web::post().to(enqueue))
            .route(web::delete().to(dequeue)),
    );
}

async fn queue_snapshot(
    request: HttpRequest,
    state: web::Data<AppState>,
) -> Result<HttpResponse, ApiError> {
    let session = required_session(&request, &state).await?;
    let matchmaking = state
        .matchmaking()
        .ok_or_else(ApiError::service_unavailable)?;
    let snapshot = matchmaking.queue_snapshot(session.account_id).await?;
    let response = QueueResponse::try_from(snapshot)?;
    Ok(HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoStore]))
        .json(response))
}

async fn enqueue(
    request: HttpRequest,
    state: web::Data<AppState>,
    #[cfg(feature = "engine")] server: Option<web::Data<Addr<Server>>>,
) -> Result<web::Json<QueueResponse>, ApiError> {
    if !state.matchmaking_enabled() {
        return Err(ApiError::service_unavailable());
    }
    let session = required_session(&request, &state).await?;
    let matchmaking = state
        .matchmaking()
        .cloned()
        .ok_or_else(ApiError::service_unavailable)?;
    #[cfg(feature = "engine")]
    if let Some(server) = server {
        let catalog = state
            .engine_catalog()
            .cloned()
            .ok_or_else(ApiError::service_unavailable)?;
        matchmaking
            .bind_runtime(Arc::new(EngineMatchWorldRuntime::new(
                server.get_ref().clone(),
                Arc::downgrade(&matchmaking),
                catalog,
            )))
            .await?;
    }
    let queued = matchmaking.enqueue(session.account_id).await?;
    QueueResponse::try_from(queued).map(web::Json)
}

async fn dequeue(
    request: HttpRequest,
    state: web::Data<AppState>,
) -> Result<web::Json<QueueResponse>, ApiError> {
    if !state.matchmaking_enabled() {
        return Err(ApiError::service_unavailable());
    }
    let session = required_session(&request, &state).await?;
    let matchmaking = state
        .matchmaking()
        .ok_or_else(ApiError::service_unavailable)?;
    let snapshot = matchmaking.cancel(session.account_id).await?;
    QueueResponse::try_from(snapshot).map(web::Json)
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
    match_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    world_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    removed: Option<bool>,
}

impl TryFrom<QueueSnapshot> for QueueResponse {
    type Error = ApiError;

    fn try_from(snapshot: QueueSnapshot) -> Result<Self, Self::Error> {
        let enqueued_at = snapshot
            .enqueued_at
            .map(|value| value.format(&Rfc3339))
            .transpose()
            .map_err(|_| ApiError::service_unavailable())?;
        Ok(Self {
            status: match snapshot.status {
                QueueStatus::Idle => "idle",
                QueueStatus::Queued => "queued",
                QueueStatus::Preparing => "preparing",
                QueueStatus::Active => "active",
                QueueStatus::ExtractionOpen => "extractionOpen",
                QueueStatus::Settling => "settling",
            },
            position: snapshot.position,
            enqueued_at,
            match_id: snapshot.match_id,
            world_name: snapshot.world_name,
            removed: snapshot.removed,
        })
    }
}
