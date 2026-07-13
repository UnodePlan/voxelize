use std::{future::Future, sync::Arc, time::Instant};

use actix_web::{http::header, web, HttpRequest, HttpResponse};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use super::OpsHttpState;
use crate::ops::{
    audit::{OpsAuditEvent, OpsAuditOutcome, OpsAuditSink, OpsOperation},
    config::hash_token,
    error::OpsApiError,
    model::{OpsPage, OpsPageRequest, DEFAULT_PAGE_LIMIT},
    repository::OpsRepositoryError,
};

pub(super) fn begin(
    request: &HttpRequest,
    state: &OpsHttpState,
    operation: OpsOperation,
) -> Result<AuditContext, HttpResponse> {
    let context = AuditContext::new(request, state.audit.clone(), operation);
    if !state.rate_limiter.allow(request.peer_addr()) {
        return Err(context.error(OpsApiError::RateLimited, OpsAuditOutcome::RateLimited));
    }
    if !authorized(request, state.token_hash) {
        return Err(context.error(OpsApiError::Unauthorized, OpsAuditOutcome::Unauthorized));
    }
    Ok(context)
}

pub(super) async fn respond_optional<T, F>(
    context: AuditContext,
    state: &OpsHttpState,
    future: F,
) -> HttpResponse
where
    T: Serialize,
    F: Future<Output = Result<Option<T>, OpsRepositoryError>>,
{
    match tokio::time::timeout(state.query_timeout, future).await {
        Ok(Ok(Some(value))) => context.success(value, 1),
        Ok(Ok(None)) => context.error(OpsApiError::NotFound, OpsAuditOutcome::NotFound),
        Ok(Err(_)) | Err(_) => {
            context.error(OpsApiError::Unavailable, OpsAuditOutcome::Unavailable)
        }
    }
}

pub(super) async fn respond_page<T, F>(
    context: AuditContext,
    state: &OpsHttpState,
    future: F,
) -> HttpResponse
where
    T: Serialize,
    F: Future<Output = Result<OpsPage<T>, OpsRepositoryError>>,
{
    match tokio::time::timeout(state.query_timeout, future).await {
        Ok(Ok(page)) => {
            let count = page.items.len();
            context.success(page, count)
        }
        Ok(Err(_)) | Err(_) => {
            context.error(OpsApiError::Unavailable, OpsAuditOutcome::Unavailable)
        }
    }
}

fn authorized(request: &HttpRequest, expected: [u8; 32]) -> bool {
    let values = request.headers().get_all(header::AUTHORIZATION);
    let mut values = values.into_iter();
    let Some(value) = values.next() else {
        return false;
    };
    if values.next().is_some() {
        return false;
    }
    let Ok(value) = value.to_str() else {
        return false;
    };
    let Some(token) = value.strip_prefix("Bearer ") else {
        return false;
    };
    constant_time_equal(hash_token(token), expected)
}

fn constant_time_equal(left: [u8; 32], right: [u8; 32]) -> bool {
    left.into_iter()
        .zip(right)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        })
        == 0
}

pub(super) fn path_uuid(request: &HttpRequest, name: &str) -> Option<Uuid> {
    Uuid::parse_str(request.match_info().get(name)?).ok()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageParams {
    limit: Option<u16>,
    offset: Option<u32>,
}

pub(super) fn page_request(request: &HttpRequest) -> Option<OpsPageRequest> {
    let params = web::Query::<PageParams>::from_query(request.query_string()).ok()?;
    OpsPageRequest::new(
        params.limit.unwrap_or(DEFAULT_PAGE_LIMIT),
        params.offset.unwrap_or(0),
    )
}

pub(super) fn operation_for_path(path: &str) -> OpsOperation {
    if path.ends_with("/participants") {
        OpsOperation::Participants
    } else if path.ends_with("/warehouse") {
        OpsOperation::Warehouse
    } else if path.ends_with("/ledger") {
        OpsOperation::Ledger
    } else if path.contains("/settlements/") {
        OpsOperation::Settlement
    } else if path.contains("/matches/") {
        OpsOperation::Match
    } else if path.contains("/accounts/") {
        OpsOperation::Account
    } else {
        OpsOperation::UnknownRoute
    }
}

pub(super) struct AuditContext {
    event: OpsAuditEvent,
    started_at: Instant,
    sink: Arc<dyn OpsAuditSink>,
}

impl AuditContext {
    fn new(request: &HttpRequest, sink: Arc<dyn OpsAuditSink>, operation: OpsOperation) -> Self {
        Self {
            event: OpsAuditEvent {
                request_id: Uuid::new_v4(),
                recorded_at: OffsetDateTime::now_utc(),
                operation,
                outcome: OpsAuditOutcome::Unavailable,
                method: request.method().as_str().to_owned(),
                status: 500,
                returned_count: None,
                duration_ms: 0,
            },
            started_at: Instant::now(),
            sink,
        }
    }

    fn success<T: Serialize>(self, value: T, count: usize) -> HttpResponse {
        let response = HttpResponse::Ok()
            .insert_header((header::CACHE_CONTROL, "no-store"))
            .insert_header((header::X_CONTENT_TYPE_OPTIONS, "nosniff"))
            .json(value);
        self.finish(OpsAuditOutcome::Success, 200, Some(count));
        response
    }

    pub(super) fn error(self, error: OpsApiError, outcome: OpsAuditOutcome) -> HttpResponse {
        let response = error.response();
        self.finish(outcome, error.status().as_u16(), None);
        response
    }

    fn finish(mut self, outcome: OpsAuditOutcome, status: u16, returned_count: Option<usize>) {
        self.event.outcome = outcome;
        self.event.status = status;
        self.event.returned_count = returned_count;
        self.event.duration_ms = self
            .started_at
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        self.sink.record(self.event);
    }
}
