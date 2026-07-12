use std::{io, sync::Arc};

use actix_web::web;

use crate::{
    auth::{AuthService, SecureAuthRandom, SiweSignatureVerifier},
    contracts,
    matchmaking::MatchmakingQueue,
    persistence::PgRepository,
    ports::{AuthRepository, Clock, RepositoryProbe, SystemClock},
    AppState, ServerConfig,
};

pub(crate) struct Application {
    pub state: web::Data<AppState>,
    #[cfg(feature = "engine")]
    pub auth: AuthService,
}

pub(crate) async fn build(config: &ServerConfig) -> io::Result<Application> {
    let repository = Arc::new(
        PgRepository::connect(config.database_url())
            .await
            .map_err(io_other)?,
    );
    let repository_probe: Arc<dyn RepositoryProbe> = repository.clone();
    let auth_repository: Arc<dyn AuthRepository> = repository;
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::default());
    let verifier = Arc::new(SiweSignatureVerifier::new(config.auth().clone()));
    let auth = AuthService::new(
        auth_repository,
        verifier,
        clock.clone(),
        Arc::new(SecureAuthRandom),
        config.auth().clone(),
    );
    let manifest = contracts::bundled_manifest()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let state = AppState::new(repository_probe, manifest)
        .with_services(auth.clone(), Arc::new(MatchmakingQueue::default()), clock)
        .with_feature_flags(config.auth_login_enabled(), config.matchmaking_enabled());
    Ok(Application {
        state: web::Data::new(state),
        #[cfg(feature = "engine")]
        auth,
    })
}

fn io_other(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::other(error)
}
