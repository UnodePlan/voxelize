use std::fmt;

use actix_web::{
    http::{
        header::{CacheControl, CacheDirective},
        StatusCode,
    },
    HttpResponse, ResponseError,
};
use serde::Serialize;

use crate::{auth::AuthError, contracts::ErrorCode, matchmaking::MatchmakingError};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ApiError {
    status: StatusCode,
    code: ErrorCode,
    retryable: bool,
}

impl ApiError {
    pub fn request_malformed() -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            code: ErrorCode::RequestMalformed,
            retryable: false,
        }
    }

    pub fn service_unavailable() -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            code: ErrorCode::ServiceUnavailable,
            retryable: true,
        }
    }
}

impl From<AuthError> for ApiError {
    fn from(error: AuthError) -> Self {
        let status = match error {
            AuthError::Required | AuthError::InvalidSiwe | AuthError::NonceInvalid => {
                StatusCode::UNAUTHORIZED
            }
            AuthError::WrongNetwork => StatusCode::UNPROCESSABLE_ENTITY,
            AuthError::ServiceUnavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        Self {
            status,
            code: error.code(),
            retryable: error == AuthError::ServiceUnavailable,
        }
    }
}

impl From<MatchmakingError> for ApiError {
    fn from(error: MatchmakingError) -> Self {
        let (status, code, retryable) = match error {
            MatchmakingError::ConnectionRequired | MatchmakingError::RosterLocked => {
                (StatusCode::CONFLICT, ErrorCode::MatchRosterLocked, false)
            }
            MatchmakingError::Full => (StatusCode::CONFLICT, ErrorCode::MatchFull, false),
            MatchmakingError::ReconnectExpired => (
                StatusCode::CONFLICT,
                ErrorCode::MatchReconnectExpired,
                false,
            ),
            MatchmakingError::Unavailable => (
                StatusCode::SERVICE_UNAVAILABLE,
                ErrorCode::ServiceUnavailable,
                true,
            ),
        };
        Self {
            status,
            code,
            retryable,
        }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("request failed")
    }
}

impl ResponseError for ApiError {
    fn status_code(&self) -> StatusCode {
        self.status
    }

    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status)
            .insert_header(CacheControl(vec![CacheDirective::NoStore]))
            .json(ApiErrorResponse {
                error: ApiErrorBody {
                    code: self.code,
                    retryable: self.retryable,
                },
            })
    }
}

#[derive(Serialize)]
struct ApiErrorResponse {
    error: ApiErrorBody,
}

#[derive(Serialize)]
struct ApiErrorBody {
    code: ErrorCode,
    retryable: bool,
}
