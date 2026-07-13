mod auth;
#[cfg(all(feature = "e2e-control", feature = "engine"))]
mod e2e_resources;
#[cfg(feature = "e2e-control")]
mod e2e_settlement;
mod error;
mod health;
mod matchmaking;
mod results;
mod session;
mod warehouse;

use std::{sync::Arc, time::Duration};

use actix_web::web;

#[cfg(feature = "engine")]
use crate::engine_catalog::EngineCatalog;
#[cfg(feature = "e2e-control")]
use crate::persistence::PgRepository;
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
    #[cfg(feature = "engine")]
    engine_catalog: Option<Arc<EngineCatalog>>,
    #[cfg(feature = "e2e-control")]
    e2e_repository: Option<Arc<PgRepository>>,
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
            #[cfg(feature = "engine")]
            engine_catalog: None,
            #[cfg(feature = "e2e-control")]
            e2e_repository: None,
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

    #[cfg(feature = "engine")]
    pub(crate) fn with_engine_catalog(mut self, catalog: Arc<EngineCatalog>) -> Self {
        self.engine_catalog = Some(catalog);
        self
    }

    #[cfg(feature = "e2e-control")]
    pub(crate) fn with_e2e_repository(mut self, repository: Arc<PgRepository>) -> Self {
        self.e2e_repository = Some(repository);
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

    #[cfg(feature = "engine")]
    pub(crate) fn engine_catalog(&self) -> Option<&Arc<EngineCatalog>> {
        self.engine_catalog.as_ref()
    }

    #[cfg(feature = "e2e-control")]
    pub(crate) fn e2e_repository(&self) -> Option<&Arc<PgRepository>> {
        self.e2e_repository.as_ref()
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
        .configure(matchmaking::configure)
        .configure(results::configure);
    #[cfg(all(feature = "e2e-control", feature = "engine"))]
    config.configure(e2e_resources::configure);
    #[cfg(feature = "e2e-control")]
    config.configure(e2e_settlement::configure);
}
