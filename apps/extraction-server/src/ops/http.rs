mod support;

use std::{sync::Arc, time::Duration};

use actix_web::{http::Method, web, HttpRequest, HttpResponse};

use super::{
    audit::{OpsAuditOutcome, OpsAuditSink, OpsOperation},
    config::hash_token,
    error::OpsApiError,
    rate_limit::OpsRateLimiter,
    repository::OpsRepository,
};
use support::{begin, operation_for_path, page_request, path_uuid, respond_optional, respond_page};

const MIN_TOKEN_BYTES: usize = 32;
const DEFAULT_QUERY_TIMEOUT: Duration = Duration::from_secs(2);
const DEFAULT_REQUESTS_PER_MINUTE: u32 = 60;
const MAX_QUERY_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REQUESTS_PER_MINUTE: u32 = 1_000;

#[derive(Clone)]
pub struct OpsHttpState {
    repository: Arc<dyn OpsRepository>,
    token_hash: [u8; 32],
    query_timeout: Duration,
    rate_limiter: OpsRateLimiter,
    audit: Arc<dyn OpsAuditSink>,
}

impl OpsHttpState {
    pub fn new(
        repository: Arc<dyn OpsRepository>,
        token: &str,
        audit: Arc<dyn OpsAuditSink>,
    ) -> Result<Self, OpsStateError> {
        if token.len() < MIN_TOKEN_BYTES {
            return Err(OpsStateError::WeakToken);
        }
        Ok(Self::from_hash(
            repository,
            hash_token(token),
            DEFAULT_QUERY_TIMEOUT,
            DEFAULT_REQUESTS_PER_MINUTE,
            audit,
        ))
    }

    pub fn with_policy(
        mut self,
        query_timeout: Duration,
        requests_per_minute: u32,
    ) -> Result<Self, OpsStateError> {
        if query_timeout.is_zero()
            || query_timeout > MAX_QUERY_TIMEOUT
            || requests_per_minute == 0
            || requests_per_minute > MAX_REQUESTS_PER_MINUTE
        {
            return Err(OpsStateError::InvalidPolicy);
        }
        self.query_timeout = query_timeout;
        self.rate_limiter = OpsRateLimiter::per_minute(requests_per_minute);
        Ok(self)
    }

    pub(crate) fn from_hash(
        repository: Arc<dyn OpsRepository>,
        token_hash: [u8; 32],
        query_timeout: Duration,
        requests_per_minute: u32,
        audit: Arc<dyn OpsAuditSink>,
    ) -> Self {
        Self {
            repository,
            token_hash,
            query_timeout,
            rate_limiter: OpsRateLimiter::per_minute(requests_per_minute),
            audit,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpsStateError {
    InvalidPolicy,
    WeakToken,
}

pub fn configure_ops(config: &mut web::ServiceConfig) {
    config
        .app_data(web::PayloadConfig::new(1_024))
        .service(
            web::scope("/ops/v1")
                .service(read_resource("/accounts/{id}", web::get().to(account)))
                .service(read_resource("/matches/{id}", web::get().to(match_record)))
                .service(read_resource(
                    "/matches/{id}/participants",
                    web::get().to(participants),
                ))
                .service(read_resource(
                    "/settlements/{id}",
                    web::get().to(settlement),
                ))
                .service(read_resource(
                    "/accounts/{id}/warehouse",
                    web::get().to(warehouse),
                ))
                .service(read_resource(
                    "/accounts/{id}/ledger",
                    web::get().to(ledger),
                ))
                .default_service(web::route().to(reject_unknown)),
        )
        .default_service(web::route().to(reject_unknown));
}

fn read_resource(path: &'static str, route: actix_web::Route) -> actix_web::Resource {
    web::resource(path)
        .route(route)
        .route(web::route().to(reject_method))
}

async fn account(request: HttpRequest, state: web::Data<OpsHttpState>) -> HttpResponse {
    let context = match begin(&request, &state, OpsOperation::Account) {
        Ok(context) => context,
        Err(response) => return response,
    };
    let Some(account_id) = path_uuid(&request, "id") else {
        return context.error(OpsApiError::InvalidRequest, OpsAuditOutcome::InvalidRequest);
    };
    let repository = state.repository.clone();
    respond_optional(context, &state, repository.account(account_id)).await
}

async fn match_record(request: HttpRequest, state: web::Data<OpsHttpState>) -> HttpResponse {
    let context = match begin(&request, &state, OpsOperation::Match) {
        Ok(context) => context,
        Err(response) => return response,
    };
    let Some(match_id) = path_uuid(&request, "id") else {
        return context.error(OpsApiError::InvalidRequest, OpsAuditOutcome::InvalidRequest);
    };
    let repository = state.repository.clone();
    respond_optional(context, &state, repository.match_record(match_id)).await
}

async fn participants(request: HttpRequest, state: web::Data<OpsHttpState>) -> HttpResponse {
    let context = match begin(&request, &state, OpsOperation::Participants) {
        Ok(context) => context,
        Err(response) => return response,
    };
    let (Some(match_id), Some(page)) = (path_uuid(&request, "id"), page_request(&request)) else {
        return context.error(OpsApiError::InvalidRequest, OpsAuditOutcome::InvalidRequest);
    };
    let repository = state.repository.clone();
    respond_page(context, &state, repository.participants(match_id, page)).await
}

async fn settlement(request: HttpRequest, state: web::Data<OpsHttpState>) -> HttpResponse {
    let context = match begin(&request, &state, OpsOperation::Settlement) {
        Ok(context) => context,
        Err(response) => return response,
    };
    let Some(settlement_id) = path_uuid(&request, "id") else {
        return context.error(OpsApiError::InvalidRequest, OpsAuditOutcome::InvalidRequest);
    };
    let repository = state.repository.clone();
    respond_optional(context, &state, repository.settlement(settlement_id)).await
}

async fn warehouse(request: HttpRequest, state: web::Data<OpsHttpState>) -> HttpResponse {
    let context = match begin(&request, &state, OpsOperation::Warehouse) {
        Ok(context) => context,
        Err(response) => return response,
    };
    let Some(account_id) = path_uuid(&request, "id") else {
        return context.error(OpsApiError::InvalidRequest, OpsAuditOutcome::InvalidRequest);
    };
    let repository = state.repository.clone();
    respond_optional(context, &state, repository.warehouse(account_id)).await
}

async fn ledger(request: HttpRequest, state: web::Data<OpsHttpState>) -> HttpResponse {
    let context = match begin(&request, &state, OpsOperation::Ledger) {
        Ok(context) => context,
        Err(response) => return response,
    };
    let (Some(account_id), Some(page)) = (path_uuid(&request, "id"), page_request(&request)) else {
        return context.error(OpsApiError::InvalidRequest, OpsAuditOutcome::InvalidRequest);
    };
    let repository = state.repository.clone();
    respond_page(context, &state, repository.ledger(account_id, page)).await
}

async fn reject_method(request: HttpRequest, state: web::Data<OpsHttpState>) -> HttpResponse {
    reject(request, state, OpsApiError::MethodNotAllowed).await
}

async fn reject_unknown(request: HttpRequest, state: web::Data<OpsHttpState>) -> HttpResponse {
    let error = if request.method() == Method::GET {
        OpsApiError::NotFound
    } else {
        OpsApiError::MethodNotAllowed
    };
    reject(request, state, error).await
}

async fn reject(
    request: HttpRequest,
    state: web::Data<OpsHttpState>,
    error: OpsApiError,
) -> HttpResponse {
    let operation = operation_for_path(request.path());
    let context = match begin(&request, &state, operation) {
        Ok(context) => context,
        Err(response) => return response,
    };
    let outcome = if error == OpsApiError::MethodNotAllowed {
        OpsAuditOutcome::MethodRejected
    } else {
        OpsAuditOutcome::NotFound
    };
    context.error(error, outcome)
}
