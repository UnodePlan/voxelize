use actix_web::{
    http::{
        header::{self, CacheControl, CacheDirective},
        StatusCode,
    },
    HttpResponse,
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum OpsApiError {
    InvalidRequest,
    MethodNotAllowed,
    NotFound,
    RateLimited,
    Unauthorized,
    Unavailable,
}

impl OpsApiError {
    pub(super) fn status(self) -> StatusCode {
        match self {
            Self::InvalidRequest => StatusCode::BAD_REQUEST,
            Self::MethodNotAllowed => StatusCode::METHOD_NOT_ALLOWED,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    pub(super) fn response(self) -> HttpResponse {
        let status = self.status();
        let mut response = HttpResponse::build(status);
        response.insert_header(CacheControl(vec![CacheDirective::NoStore]));
        response.insert_header((header::X_CONTENT_TYPE_OPTIONS, "nosniff"));
        if self == Self::Unauthorized {
            response.insert_header((header::WWW_AUTHENTICATE, "Bearer"));
        }
        if self == Self::RateLimited {
            response.insert_header((header::RETRY_AFTER, "60"));
        }
        response.json(OpsErrorResponse {
            error: OpsErrorBody { code: self.code() },
        })
    }

    fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "OPS_REQUEST_INVALID",
            Self::MethodNotAllowed => "OPS_READ_ONLY",
            Self::NotFound => "OPS_NOT_FOUND",
            Self::RateLimited => "OPS_RATE_LIMITED",
            Self::Unauthorized => "OPS_AUTH_REQUIRED",
            Self::Unavailable => "OPS_UNAVAILABLE",
        }
    }
}

#[derive(Serialize)]
struct OpsErrorResponse {
    error: OpsErrorBody,
}

#[derive(Serialize)]
struct OpsErrorBody {
    code: &'static str,
}
