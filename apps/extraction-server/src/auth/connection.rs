use voxelize::{
    ConnectionAuthError, ConnectionAuthRequest, ConnectionAuthenticator, ConnectionPrincipal,
};

use super::{cookie_value_from_headers, AuthError, AuthService};

#[derive(Clone)]
pub struct SessionConnectionAuthenticator {
    auth: AuthService,
}

impl SessionConnectionAuthenticator {
    pub fn new(auth: AuthService) -> Self {
        Self { auth }
    }
}

impl ConnectionAuthenticator for SessionConnectionAuthenticator {
    fn authenticate(&self, request: ConnectionAuthRequest) -> voxelize::ConnectionAuthFuture {
        let auth = self.auth.clone();
        Box::pin(async move {
            let token = cookie_value_from_headers(&request.headers, auth.config().cookie_name())
                .ok_or_else(|| ConnectionAuthError::new("auth_required"))?;
            let session = auth
                .authenticate_token(&token)
                .await
                .map_err(map_auth_error)?;
            Ok(ConnectionPrincipal::new(
                session.account_id.to_string(),
                session.session_id.to_string(),
            )
            .with_valid_until(session.valid_until().into()))
        })
    }

    fn revalidate(&self, request: ConnectionAuthRequest) -> voxelize::ConnectionAuthFuture {
        let auth = self.auth.clone();
        Box::pin(async move {
            let token = cookie_value_from_headers(&request.headers, auth.config().cookie_name())
                .ok_or_else(|| ConnectionAuthError::new("auth_required"))?;
            let session = auth.inspect_token(&token).await.map_err(map_auth_error)?;
            Ok(ConnectionPrincipal::new(
                session.account_id.to_string(),
                session.session_id.to_string(),
            )
            .with_valid_until(session.valid_until().into()))
        })
    }
}

fn map_auth_error(error: AuthError) -> ConnectionAuthError {
    match error {
        AuthError::ServiceUnavailable => ConnectionAuthError::unavailable(),
        _ => ConnectionAuthError::new("auth_required"),
    }
}
