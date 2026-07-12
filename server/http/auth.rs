use std::{fmt, future::Future, net::SocketAddr, pin::Pin, time::SystemTime};

use actix_web::http::header::HeaderMap;

/// Authenticated identity attached to a WebSocket connection.
///
/// These identifiers are server-issued and must never be populated from public
/// query parameters. The engine uses a separate random connection ID for routing.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionPrincipal {
    pub account_id: String,
    pub session_id: String,
    valid_until: Option<SystemTime>,
}

impl ConnectionPrincipal {
    pub fn new(account_id: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            account_id: account_id.into(),
            session_id: session_id.into(),
            valid_until: None,
        }
    }

    pub fn with_valid_until(mut self, valid_until: SystemTime) -> Self {
        self.valid_until = Some(valid_until);
        self
    }

    pub(crate) fn is_valid(&self) -> bool {
        !self.account_id.trim().is_empty() && !self.session_id.trim().is_empty()
    }

    pub(crate) fn valid_until(&self) -> Option<SystemTime> {
        self.valid_until
    }

    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        self.account_id == other.account_id && self.session_id == other.session_id
    }

    pub(crate) fn is_expired_at(&self, now: SystemTime) -> bool {
        self.valid_until.is_some_and(|deadline| deadline <= now)
    }
}

/// Owned request data supplied to an asynchronous connection authenticator.
#[derive(Clone)]
pub struct ConnectionAuthRequest {
    pub headers: HeaderMap,
    pub peer_addr: Option<SocketAddr>,
    pub path: String,
}

pub type ConnectionAuthFuture = Pin<
    Box<dyn Future<Output = Result<ConnectionPrincipal, ConnectionAuthError>> + Send + 'static>,
>;

/// Object-safe asynchronous authentication boundary used by public servers.
pub trait ConnectionAuthenticator: Send + Sync {
    fn authenticate(&self, request: ConnectionAuthRequest) -> ConnectionAuthFuture;

    /// Re-check a connection without treating the check itself as user activity.
    fn revalidate(&self, request: ConnectionAuthRequest) -> ConnectionAuthFuture {
        self.authenticate(request)
    }
}

impl<F, Fut> ConnectionAuthenticator for F
where
    F: Fn(ConnectionAuthRequest) -> Fut + Send + Sync,
    Fut: Future<Output = Result<ConnectionPrincipal, ConnectionAuthError>> + Send + 'static,
{
    fn authenticate(&self, request: ConnectionAuthRequest) -> ConnectionAuthFuture {
        Box::pin((self)(request))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionAuthErrorKind {
    Unauthorized,
    Unavailable,
}

/// Stable authentication failure. The code is for application telemetry and is
/// deliberately not returned in the public HTTP response.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConnectionAuthError {
    pub code: String,
    pub kind: ConnectionAuthErrorKind,
}

impl ConnectionAuthError {
    pub fn new(code: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            kind: ConnectionAuthErrorKind::Unauthorized,
        }
    }

    pub fn unavailable() -> Self {
        Self {
            code: "connection_auth_unavailable".to_owned(),
            kind: ConnectionAuthErrorKind::Unavailable,
        }
    }
}

impl fmt::Display for ConnectionAuthError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.code)
    }
}

impl std::error::Error for ConnectionAuthError {}
