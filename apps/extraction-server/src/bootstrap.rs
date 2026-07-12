use std::{io, sync::Arc};

use actix_web::web;

use crate::{
    auth::{AuthService, SecureAuthRandom, SiweSignatureVerifier},
    contracts,
    matchmaking::{MatchVersions, MatchmakingService},
    persistence::{acquire_matchmaking_process_lock, MatchmakingProcessLock, PgRepository},
    ports::{
        AuthRepository, Clock, MatchRepository, RandomIdGenerator, RandomSeedGenerator,
        RepositoryProbe, SystemClock,
    },
    AppState, ServerConfig,
};

const STARTUP_ABORT_REASON: &str = "process_restart";

pub(crate) struct Application {
    pub state: web::Data<AppState>,
    pub matchmaking_process_lock: MatchmakingProcessLock,
    #[cfg(feature = "engine")]
    pub matchmaking: Arc<MatchmakingService>,
    #[cfg(feature = "engine")]
    pub auth: AuthService,
}

pub(crate) async fn build(config: &ServerConfig) -> io::Result<Application> {
    let matchmaking_process_lock = acquire_matchmaking_process_lock(config.database_url())
        .await
        .map_err(io_other)?
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::AddrInUse,
                "another matchmaking process already owns this database",
            )
        })?;
    let repository = Arc::new(
        PgRepository::connect(config.database_url())
            .await
            .map_err(io_other)?,
    );
    let repository_probe: Arc<dyn RepositoryProbe> = repository.clone();
    let auth_repository: Arc<dyn AuthRepository> = repository.clone();
    let match_repository: Arc<dyn MatchRepository> = repository;
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::default());
    match_repository
        .abort_unrecoverable_matches(STARTUP_ABORT_REASON.to_owned(), clock.utc_now().into())
        .await
        .map_err(|error| {
            io::Error::other(format!(
                "failed to abort unrecoverable matches during startup: {error:?}"
            ))
        })?;
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
    let matchmaking = MatchmakingService::start(
        match_repository,
        clock.clone(),
        Arc::new(RandomIdGenerator),
        Arc::new(RandomSeedGenerator),
        MatchVersions {
            generation: manifest.generation_version.clone(),
            gameplay: manifest.gameplay_version.clone(),
            config: manifest.config_version.clone(),
        },
    );
    matchmaking.start_ticker();
    let state = AppState::new(repository_probe, manifest)
        .with_services(auth.clone(), matchmaking.clone(), clock)
        .with_feature_flags(config.auth_login_enabled(), config.matchmaking_enabled());
    Ok(Application {
        state: web::Data::new(state),
        matchmaking_process_lock,
        #[cfg(feature = "engine")]
        matchmaking,
        #[cfg(feature = "engine")]
        auth,
    })
}

fn io_other(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::other(error)
}
