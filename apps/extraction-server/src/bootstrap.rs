use std::{io, sync::Arc};

use actix_web::web;

#[cfg(feature = "engine")]
use crate::engine_catalog::EngineCatalog;
use crate::observability::{MatchEvent, MatchEventSink, RecoveryOutcome, StderrMatchEventSink};
use crate::{
    auth::{AuthService, SecureAuthRandom, SiweSignatureVerifier},
    contracts,
    matchmaking::{MatchVersions, MatchmakingService},
    persistence::{acquire_matchmaking_process_lock, MatchmakingProcessLock, PgRepository},
    ports::{
        AuthRepository, Clock, MatchmakingRepository, RandomIdGenerator, RandomSeedGenerator,
        RepositoryProbe,
    },
    AppState, ServerConfig,
};

const STARTUP_ABORT_REASON: &str = "process_restart";

pub(crate) struct Application {
    pub state: web::Data<AppState>,
    pub matchmaking_process_lock: MatchmakingProcessLock,
    #[cfg(feature = "engine")]
    pub engine_catalog: Arc<EngineCatalog>,
    #[cfg(feature = "engine")]
    pub matchmaking: Arc<MatchmakingService>,
    #[cfg(feature = "engine")]
    pub auth: AuthService,
}

pub(crate) async fn build(config: &ServerConfig, clock: Arc<dyn Clock>) -> io::Result<Application> {
    #[cfg(feature = "e2e-control")]
    crate::persistence::validate_e2e_settlement_crash_configuration()?;
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
    let match_repository: Arc<dyn MatchmakingRepository> = repository.clone();
    match match_repository
        .abort_unrecoverable_matches(STARTUP_ABORT_REASON.to_owned(), clock.utc_now().into())
        .await
    {
        Ok(affected_matches) => StderrMatchEventSink.record(MatchEvent::StartupRecovery {
            outcome: RecoveryOutcome::Completed,
            affected_matches,
        }),
        Err(error) => {
            StderrMatchEventSink.record(MatchEvent::StartupRecovery {
                outcome: RecoveryOutcome::Failed,
                affected_matches: 0,
            });
            return Err(io::Error::other(format!(
                "failed to abort unrecoverable matches during startup: {error:?}"
            )));
        }
    }
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
    #[cfg(feature = "engine")]
    let engine_catalog = Arc::new(
        EngineCatalog::from_manifest(&manifest)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?,
    );
    let matchmaking = MatchmakingService::start_with_match_size(
        match_repository,
        clock.clone(),
        Arc::new(RandomIdGenerator),
        Arc::new(RandomSeedGenerator),
        MatchVersions {
            generation: manifest.generation_version.clone(),
            gameplay: manifest.gameplay_version.clone(),
            config: manifest.config_version.clone(),
        },
        config.match_size(),
    );
    matchmaking.start_ticker();
    let state = AppState::new(repository_probe, manifest)
        .with_services(auth.clone(), matchmaking.clone(), clock)
        .with_feature_flags(config.auth_login_enabled(), config.matchmaking_enabled());
    #[cfg(feature = "e2e-control")]
    let state = state.with_e2e_repository(repository);
    #[cfg(feature = "engine")]
    let state = state.with_engine_catalog(engine_catalog.clone());
    Ok(Application {
        state: web::Data::new(state),
        matchmaking_process_lock,
        #[cfg(feature = "engine")]
        engine_catalog,
        #[cfg(feature = "engine")]
        matchmaking,
        #[cfg(feature = "engine")]
        auth,
    })
}

fn io_other(error: impl std::error::Error + Send + Sync + 'static) -> io::Error {
    io::Error::other(error)
}
