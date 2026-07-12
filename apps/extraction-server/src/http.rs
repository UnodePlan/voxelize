mod auth;
mod error;
mod health;
mod matchmaking;
mod session;
mod warehouse;

use std::{sync::Arc, time::Duration};

use actix_web::web;

use crate::{
    auth::{AuthService, NonceRateLimiter},
    contracts::ExtractionManifest,
    matchmaking::MatchmakingService,
    ports::{Clock, RepositoryProbe, SystemClock},
};

const DEFAULT_READINESS_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_JSON_BODY_SIZE: usize = 16 * 1024;

#[derive(Clone)]
pub struct AppState {
    repository: Arc<dyn RepositoryProbe>,
    manifest: ExtractionManifest,
    readiness_timeout: Duration,
    auth: Option<AuthService>,
    matchmaking: Option<Arc<MatchmakingService>>,
    clock: Arc<dyn Clock>,
    auth_login_enabled: bool,
    matchmaking_enabled: bool,
    nonce_rate_limiter: NonceRateLimiter,
    verification_rate_limiter: NonceRateLimiter,
}

impl AppState {
    pub fn new(repository: Arc<dyn RepositoryProbe>, manifest: ExtractionManifest) -> Self {
        Self {
            repository,
            manifest,
            readiness_timeout: DEFAULT_READINESS_TIMEOUT,
            auth: None,
            matchmaking: None,
            clock: Arc::new(SystemClock::default()),
            auth_login_enabled: true,
            matchmaking_enabled: true,
            nonce_rate_limiter: NonceRateLimiter::default(),
            verification_rate_limiter: NonceRateLimiter::default(),
        }
    }

    pub fn with_services(
        mut self,
        auth: AuthService,
        matchmaking: Arc<MatchmakingService>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        self.auth = Some(auth);
        self.matchmaking = Some(matchmaking);
        self.clock = clock;
        self
    }

    pub fn with_feature_flags(
        mut self,
        auth_login_enabled: bool,
        matchmaking_enabled: bool,
    ) -> Self {
        self.auth_login_enabled = auth_login_enabled;
        self.matchmaking_enabled = matchmaking_enabled;
        self
    }

    pub fn with_readiness_timeout(mut self, readiness_timeout: Duration) -> Self {
        self.readiness_timeout = readiness_timeout;
        self
    }

    pub(crate) fn auth(&self) -> Option<&AuthService> {
        self.auth.as_ref()
    }

    pub(crate) fn matchmaking(&self) -> Option<&Arc<MatchmakingService>> {
        self.matchmaking.as_ref()
    }

    pub(crate) fn auth_login_enabled(&self) -> bool {
        self.auth_login_enabled
    }

    pub(crate) fn matchmaking_enabled(&self) -> bool {
        self.matchmaking_enabled
    }

    pub(crate) fn allow_nonce_request(&self, peer_addr: Option<std::net::SocketAddr>) -> bool {
        self.nonce_rate_limiter
            .allow(peer_addr, self.clock.monotonic_now())
    }

    pub(crate) fn allow_verification_request(
        &self,
        peer_addr: Option<std::net::SocketAddr>,
    ) -> bool {
        self.verification_rate_limiter
            .allow(peer_addr, self.clock.monotonic_now())
    }
}

pub use health::HealthResponse;

pub fn configure_api(config: &mut web::ServiceConfig) {
    config
        .app_data(
            web::JsonConfig::default()
                .limit(MAX_JSON_BODY_SIZE)
                .error_handler(|_, _| error::ApiError::request_malformed().into()),
        )
        .configure(health::configure)
        .configure(auth::configure)
        .configure(warehouse::configure)
        .configure(matchmaking::configure);
}
