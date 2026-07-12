mod auth;
mod origin;

use std::{sync::Arc, time::Duration};

use actix_web::web;

pub use auth::*;
pub(crate) use origin::strict_origin_guard;
use origin::valid_exact_origin;

const DEFAULT_MAX_WS_MESSAGE_SIZE: usize = 16 * 1024 * 1024;
const DEFAULT_MAX_HTTP_PAYLOAD_SIZE: usize = 1024 * 1024;
const DEFAULT_OUTBOUND_QUEUE_CAPACITY: usize = 1024;
const DEFAULT_WORLD_REQUEST_CAPACITY: usize = 1024;
const DEFAULT_CLIENT_MESSAGE_TIMEOUT: Duration = Duration::from_secs(1);
const DEFAULT_AUTH_TIMEOUT: Duration = Duration::from_secs(3);
const PUBLIC_MAX_WS_MESSAGE_SIZE: usize = 4 * 1024 * 1024;
const PUBLIC_OUTBOUND_QUEUE_CAPACITY: usize = 64;
const PUBLIC_WORLD_REQUEST_CAPACITY: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CorsPolicy {
    /// Maintains the existing demo behavior. Do not use with cookie sessions.
    Permissive,
    /// Only origins in this list may reach HTTP or WebSocket entry points.
    AllowList(Vec<String>),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ConnectionSecurityMode {
    #[default]
    Legacy,
    PublicStrict,
}

type RouteConfigurator = Arc<dyn Fn(&mut web::ServiceConfig) + Send + Sync>;

/// HTTP and WebSocket trust-boundary configuration.
#[derive(Clone)]
pub struct HttpConfig {
    security_mode: ConnectionSecurityMode,
    cors: CorsPolicy,
    allow_missing_origin: bool,
    authenticator: Option<Arc<dyn ConnectionAuthenticator>>,
    require_authentication: bool,
    max_ws_message_size: usize,
    max_http_payload_size: usize,
    outbound_queue_capacity: usize,
    world_request_capacity: usize,
    client_message_timeout: Duration,
    auth_timeout: Duration,
    expose_info: bool,
    route_configurators: Vec<RouteConfigurator>,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self::legacy()
    }
}

impl HttpConfig {
    /// Legacy mode preserves query-parameter client IDs and permissive CORS.
    pub fn legacy() -> Self {
        Self {
            security_mode: ConnectionSecurityMode::Legacy,
            cors: CorsPolicy::Permissive,
            allow_missing_origin: true,
            authenticator: None,
            require_authentication: false,
            max_ws_message_size: DEFAULT_MAX_WS_MESSAGE_SIZE,
            max_http_payload_size: DEFAULT_MAX_HTTP_PAYLOAD_SIZE,
            outbound_queue_capacity: DEFAULT_OUTBOUND_QUEUE_CAPACITY,
            world_request_capacity: DEFAULT_WORLD_REQUEST_CAPACITY,
            client_message_timeout: DEFAULT_CLIENT_MESSAGE_TIMEOUT,
            auth_timeout: DEFAULT_AUTH_TIMEOUT,
            expose_info: true,
            route_configurators: Vec::new(),
        }
    }

    /// Public mode ignores client-provided IDs and fails closed on auth errors.
    pub fn authenticated<A>(authenticator: A) -> Self
    where
        A: ConnectionAuthenticator + 'static,
    {
        Self {
            security_mode: ConnectionSecurityMode::PublicStrict,
            cors: CorsPolicy::AllowList(Vec::new()),
            allow_missing_origin: false,
            authenticator: Some(Arc::new(authenticator)),
            require_authentication: true,
            max_ws_message_size: PUBLIC_MAX_WS_MESSAGE_SIZE,
            outbound_queue_capacity: PUBLIC_OUTBOUND_QUEUE_CAPACITY,
            world_request_capacity: PUBLIC_WORLD_REQUEST_CAPACITY,
            expose_info: false,
            ..Self::legacy()
        }
    }

    pub fn authenticated_arc(authenticator: Arc<dyn ConnectionAuthenticator>) -> Self {
        Self {
            security_mode: ConnectionSecurityMode::PublicStrict,
            cors: CorsPolicy::AllowList(Vec::new()),
            allow_missing_origin: false,
            authenticator: Some(authenticator),
            require_authentication: true,
            max_ws_message_size: PUBLIC_MAX_WS_MESSAGE_SIZE,
            outbound_queue_capacity: PUBLIC_OUTBOUND_QUEUE_CAPACITY,
            world_request_capacity: PUBLIC_WORLD_REQUEST_CAPACITY,
            expose_info: false,
            ..Self::legacy()
        }
    }

    pub fn allowed_origins<I, S>(mut self, origins: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.cors = CorsPolicy::AllowList(origins.into_iter().map(Into::into).collect());
        self
    }

    pub fn allow_missing_origin(mut self, allow: bool) -> Self {
        self.allow_missing_origin = allow;
        self
    }

    pub fn max_ws_message_size(mut self, bytes: usize) -> Self {
        self.max_ws_message_size = bytes.max(1);
        self
    }

    pub fn max_http_payload_size(mut self, bytes: usize) -> Self {
        self.max_http_payload_size = bytes.max(1);
        self
    }

    pub fn outbound_queue_capacity(mut self, capacity: usize) -> Self {
        self.outbound_queue_capacity = capacity.max(1);
        self
    }

    pub fn world_request_capacity(mut self, capacity: usize) -> Self {
        self.world_request_capacity = capacity.max(1);
        self
    }

    pub fn client_message_timeout(mut self, timeout: Duration) -> Self {
        self.client_message_timeout = timeout;
        self
    }

    pub fn auth_timeout(mut self, timeout: Duration) -> Self {
        self.auth_timeout = timeout;
        self
    }

    pub fn expose_info(mut self, expose: bool) -> Self {
        self.expose_info = expose;
        self
    }

    /// Register application-owned HTTP routes without coupling them to the engine.
    pub fn configure_routes<F>(mut self, configure: F) -> Self
    where
        F: Fn(&mut web::ServiceConfig) + Send + Sync + 'static,
    {
        self.route_configurators.push(Arc::new(configure));
        self
    }

    pub fn cors(&self) -> &CorsPolicy {
        &self.cors
    }

    pub fn security_mode(&self) -> ConnectionSecurityMode {
        self.security_mode
    }

    pub fn requires_authentication(&self) -> bool {
        self.require_authentication
    }

    pub fn max_ws_message_size_bytes(&self) -> usize {
        self.max_ws_message_size
    }

    pub fn max_http_payload_size_bytes(&self) -> usize {
        self.max_http_payload_size
    }

    pub fn outbound_queue_capacity_value(&self) -> usize {
        self.outbound_queue_capacity
    }

    pub fn world_request_capacity_value(&self) -> usize {
        self.world_request_capacity
    }

    pub fn client_message_timeout_value(&self) -> Duration {
        self.client_message_timeout
    }

    pub fn auth_timeout_value(&self) -> Duration {
        self.auth_timeout
    }

    pub fn exposes_info(&self) -> bool {
        self.expose_info
    }

    pub(crate) fn validate(&self) -> std::io::Result<()> {
        if self.security_mode != ConnectionSecurityMode::PublicStrict {
            return Ok(());
        }

        if self.authenticator.is_none() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "public HTTP mode requires a connection authenticator",
            ));
        }

        if self.allow_missing_origin {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "public HTTP mode cannot allow requests without an Origin header",
            ));
        }

        if self.expose_info {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "public HTTP mode cannot expose the engine info endpoint",
            ));
        }

        if self.auth_timeout.is_zero() || self.client_message_timeout.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "public HTTP mode requires non-zero authentication and message timeouts",
            ));
        }

        match &self.cors {
            CorsPolicy::AllowList(origins)
                if !origins.is_empty()
                    && origins.iter().all(|origin| valid_exact_origin(origin))
                    && origins
                        .iter()
                        .enumerate()
                        .all(|(index, origin)| !origins[..index].contains(origin)) =>
            {
                Ok(())
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "public HTTP mode requires valid exact http(s) origins without paths",
            )),
        }
    }

    pub(crate) fn origin_allowed(&self, origin: Option<&str>) -> bool {
        match (&self.cors, origin) {
            (CorsPolicy::Permissive, _) => true,
            (CorsPolicy::AllowList(_), None) => self.allow_missing_origin,
            (CorsPolicy::AllowList(origins), Some(origin)) => {
                origins.iter().any(|allowed| allowed == origin)
            }
        }
    }

    pub(crate) async fn authenticate(
        &self,
        request: ConnectionAuthRequest,
    ) -> Result<Option<ConnectionPrincipal>, ConnectionAuthError> {
        if !self.require_authentication {
            return Ok(None);
        }

        let authenticator = self
            .authenticator
            .as_ref()
            .ok_or_else(ConnectionAuthError::unavailable)?;
        authenticator.authenticate(request).await.map(Some)
    }

    pub(crate) fn apply_routes(&self, config: &mut web::ServiceConfig) {
        for configure in &self.route_configurators {
            configure(config);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::http::header::HeaderMap;

    #[actix_web::test]
    async fn authenticated_config_returns_server_issued_principal() {
        let config = HttpConfig::authenticated(|_: ConnectionAuthRequest| async {
            Ok(ConnectionPrincipal::new("account-1", "session-1"))
        });

        let principal = config
            .authenticate(ConnectionAuthRequest {
                headers: HeaderMap::new(),
                peer_addr: None,
                path: "/ws/".to_owned(),
            })
            .await
            .unwrap()
            .unwrap();

        assert_eq!(principal.account_id, "account-1");
        assert_eq!(principal.session_id, "session-1");
    }

    #[test]
    fn authenticated_config_denies_unlisted_or_missing_origins() {
        let config =
            HttpConfig::authenticated(|_| async { Err(ConnectionAuthError::new("unused")) })
                .allowed_origins(["https://game.example"]);

        assert!(config.origin_allowed(Some("https://game.example")));
        assert!(!config.origin_allowed(Some("https://evil.example")));
        assert!(!config.origin_allowed(None));
    }
}
