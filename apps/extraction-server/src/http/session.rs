use actix_web::{web, HttpRequest};

use crate::auth::{cookie_value_from_headers, AuthError, AuthSession};

use super::{error::ApiError, AppState};

pub(super) fn session_token(request: &HttpRequest, state: &AppState) -> Option<String> {
    let auth = state.auth()?;
    cookie_value_from_headers(request.headers(), auth.config().cookie_name())
}

pub(super) async fn optional_session(
    request: &HttpRequest,
    state: &web::Data<AppState>,
) -> Result<Option<AuthSession>, ApiError> {
    let auth = state.auth().ok_or_else(ApiError::service_unavailable)?;
    let Some(token) = session_token(request, state) else {
        return Ok(None);
    };
    match auth.authenticate_token(&token).await {
        Ok(session) => Ok(Some(session)),
        Err(AuthError::Required) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(super) async fn required_session(
    request: &HttpRequest,
    state: &web::Data<AppState>,
) -> Result<AuthSession, ApiError> {
    optional_session(request, state)
        .await?
        .ok_or_else(|| AuthError::Required.into())
}
