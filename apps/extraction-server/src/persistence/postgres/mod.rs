mod auth;
#[cfg(feature = "e2e-control")]
mod e2e_settlement_audit;
#[cfg(feature = "e2e-control")]
mod e2e_settlement_crash;
#[cfg(feature = "e2e-control")]
mod e2e_settlement_fault;
mod matchmaking;
mod process_lock;
mod session;
mod settlement_assets;
mod settlement_commit;
mod settlement_read;
mod settlement_results;
mod settlement_write;
mod warehouse;

#[cfg(feature = "e2e-control")]
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use sqlx::{migrate::Migrator, postgres::PgPoolOptions, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::matchmaking::{
    CommitSettlement, ExtractionQualification, MatchResultRecord, SettlementRecord,
};
use crate::matchmaking::{CreatePreparingMatch, ParticipantDeath, ParticipantRecord, StoredMatch};
use crate::ports::{
    AuthRepository, AuthRepositoryError, LoginCommand, LoginResult, MatchRepository,
    MatchRepositoryError, NewNonce, RepositoryError, RepositoryFuture, RepositoryProbe,
    SessionRecord, SettlementRepository, SettlementRepositoryError, SettlingTrigger, StoredNonce,
    TransitionOutcome, WarehouseSnapshot,
};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

pub use process_lock::{acquire_matchmaking_process_lock, MatchmakingProcessLock};

#[derive(Clone, Debug)]
pub struct PgRepository {
    pool: PgPool,
    #[cfg(feature = "e2e-control")]
    settlement_fault: Arc<e2e_settlement_fault::E2eSettlementFaultControl>,
}

impl PgRepository {
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(3))
            .connect(database_url)
            .await?;
        Ok(Self {
            pool,
            #[cfg(feature = "e2e-control")]
            settlement_fault: Arc::new(Default::default()),
        })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self {
            pool,
            #[cfg(feature = "e2e-control")]
            settlement_fault: Arc::new(Default::default()),
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}

#[cfg(feature = "e2e-control")]
pub(crate) use e2e_settlement_crash::validate_configuration as validate_e2e_settlement_crash_configuration;
#[cfg(feature = "e2e-control")]
pub(crate) use e2e_settlement_fault::{E2eSettlementFaultAction, E2eSettlementFaultSnapshot};

#[cfg(feature = "e2e-control")]
impl PgRepository {
    pub(crate) fn apply_e2e_settlement_fault(
        &self,
        action: E2eSettlementFaultAction,
    ) -> E2eSettlementFaultSnapshot {
        self.settlement_fault.apply(action)
    }

    pub(crate) fn e2e_settlement_fault_snapshot(&self) -> E2eSettlementFaultSnapshot {
        self.settlement_fault.snapshot()
    }
}

pub async fn migrate_database(database_url: &str) -> Result<(), sqlx::migrate::MigrateError> {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;
    MIGRATOR.run(&pool).await
}

impl RepositoryProbe for PgRepository {
    fn check(&self) -> RepositoryFuture<'_> {
        Box::pin(async move {
            sqlx::query_scalar::<_, i32>("SELECT 1")
                .fetch_one(&self.pool)
                .await
                .map(|_| ())
                .map_err(|_| RepositoryError::new("postgres repository unavailable"))
        })
    }
}

#[async_trait]
impl AuthRepository for PgRepository {
    async fn insert_nonce(&self, nonce: NewNonce) -> Result<(), AuthRepositoryError> {
        auth::insert_nonce(&self.pool, nonce).await
    }

    async fn find_nonce(
        &self,
        nonce_hash: [u8; 32],
    ) -> Result<Option<StoredNonce>, AuthRepositoryError> {
        auth::find_nonce(&self.pool, nonce_hash).await
    }

    async fn prune_expired_nonces(
        &self,
        now: OffsetDateTime,
        limit: u32,
    ) -> Result<u64, AuthRepositoryError> {
        auth::prune_expired_nonces(&self.pool, now, limit).await
    }

    async fn complete_login(
        &self,
        command: LoginCommand,
    ) -> Result<LoginResult, AuthRepositoryError> {
        auth::complete_login(&self.pool, command).await
    }

    async fn find_active_session(
        &self,
        token_hash: [u8; 32],
        now: OffsetDateTime,
    ) -> Result<Option<SessionRecord>, AuthRepositoryError> {
        session::find_active_session(&self.pool, token_hash, now).await
    }

    async fn inspect_active_session(
        &self,
        token_hash: [u8; 32],
        now: OffsetDateTime,
    ) -> Result<Option<SessionRecord>, AuthRepositoryError> {
        session::inspect_active_session(&self.pool, token_hash, now).await
    }

    async fn revoke_session(
        &self,
        token_hash: [u8; 32],
        now: OffsetDateTime,
    ) -> Result<Option<Uuid>, AuthRepositoryError> {
        session::revoke_session(&self.pool, token_hash, now).await
    }

    async fn warehouse(&self, account_id: Uuid) -> Result<WarehouseSnapshot, AuthRepositoryError> {
        warehouse::load_warehouse(&self.pool, account_id).await
    }
}

#[async_trait]
impl MatchRepository for PgRepository {
    async fn create_preparing(
        &self,
        command: CreatePreparingMatch,
    ) -> Result<StoredMatch, MatchRepositoryError> {
        matchmaking::create_preparing(&self.pool, command).await
    }

    async fn find_match(
        &self,
        match_id: Uuid,
    ) -> Result<Option<StoredMatch>, MatchRepositoryError> {
        matchmaking::find_match(&self.pool, match_id).await
    }

    async fn find_nonterminal_by_account(
        &self,
        account_id: Uuid,
    ) -> Result<Option<StoredMatch>, MatchRepositoryError> {
        matchmaking::find_nonterminal_by_account(&self.pool, account_id).await
    }

    async fn abort_unrecoverable_matches(
        &self,
        reason: String,
        at: OffsetDateTime,
    ) -> Result<u64, MatchRepositoryError> {
        matchmaking::abort_unrecoverable_matches(&self.pool, reason, at).await
    }

    async fn activate(
        &self,
        match_id: Uuid,
        started_at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
        matchmaking::activate(&self.pool, match_id, started_at).await
    }

    async fn abort(
        &self,
        match_id: Uuid,
        reason: String,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
        matchmaking::abort(&self.pool, match_id, reason, at).await
    }

    async fn mark_disconnected(
        &self,
        match_id: Uuid,
        account_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError> {
        matchmaking::mark_disconnected(&self.pool, match_id, account_id, at).await
    }

    async fn reconnect(
        &self,
        match_id: Uuid,
        account_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError> {
        matchmaking::reconnect(&self.pool, match_id, account_id, at).await
    }

    async fn mark_dead(
        &self,
        death: ParticipantDeath,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError> {
        matchmaking::mark_dead(&self.pool, death).await
    }

    async fn mark_timed_out(
        &self,
        timeout: crate::matchmaking::ParticipantTimeout,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError> {
        matchmaking::mark_timed_out(&self.pool, timeout).await
    }

    async fn open_extraction(
        &self,
        match_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
        matchmaking::open_extraction(&self.pool, match_id, at).await
    }

    async fn begin_settling(
        &self,
        match_id: Uuid,
        trigger: SettlingTrigger,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
        matchmaking::begin_settling(&self.pool, match_id, trigger, at).await
    }

    async fn finish(
        &self,
        match_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
        matchmaking::finish(&self.pool, match_id, at).await
    }
}

#[async_trait]
impl SettlementRepository for PgRepository {
    async fn mark_settlement_pending(
        &self,
        qualification: ExtractionQualification,
    ) -> Result<TransitionOutcome<ParticipantRecord>, SettlementRepositoryError> {
        #[cfg(feature = "e2e-control")]
        self.settlement_fault.record_mark();
        settlement_write::mark_pending(&self.pool, qualification).await
    }

    async fn commit_settlement(
        &self,
        command: CommitSettlement,
    ) -> Result<TransitionOutcome<SettlementRecord>, SettlementRepositoryError> {
        #[cfg(feature = "e2e-control")]
        self.settlement_fault.record_commit();
        let outcome = settlement_commit::commit(&self.pool, command).await?;
        #[cfg(feature = "e2e-control")]
        if outcome.was_applied() {
            self.settlement_fault.after_applied_commit()?;
        }
        Ok(outcome)
    }

    async fn find_settlement(
        &self,
        match_id: Uuid,
        account_id: Uuid,
    ) -> Result<Option<SettlementRecord>, SettlementRepositoryError> {
        #[cfg(feature = "e2e-control")]
        self.settlement_fault.before_settlement_read()?;
        settlement_read::find_settlement(&self.pool, match_id, account_id).await
    }

    async fn abort_pending_settlement(
        &self,
        match_id: Uuid,
        account_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<ParticipantRecord>, SettlementRepositoryError> {
        #[cfg(feature = "e2e-control")]
        self.settlement_fault.record_abort();
        settlement_write::abort_pending(&self.pool, match_id, account_id, at).await
    }

    async fn find_match_result(
        &self,
        match_id: Uuid,
        account_id: Uuid,
    ) -> Result<Option<MatchResultRecord>, SettlementRepositoryError> {
        #[cfg(feature = "e2e-control")]
        self.settlement_fault.before_result_read()?;
        settlement_results::find_match_result(&self.pool, match_id, account_id).await
    }

    async fn find_latest_match_result(
        &self,
        account_id: Uuid,
    ) -> Result<Option<MatchResultRecord>, SettlementRepositoryError> {
        #[cfg(feature = "e2e-control")]
        self.settlement_fault.before_result_read()?;
        settlement_results::find_latest_match_result(&self.pool, account_id).await
    }
}
