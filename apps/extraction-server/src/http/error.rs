use std::fmt;

use actix_web::{http::StatusCode, HttpResponse, ResponseError};
use serde::Serialize;

use crate::{auth::AuthError, contracts::ErrorCode};

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
        HttpResponse::build(self.status).json(ApiErrorResponse {
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
