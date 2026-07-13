use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use extraction_server::{
    auth::{
        AuthConfig, AuthRandom, AuthRandomError, AuthService, SignatureVerificationError,
        SignatureVerifier,
    },
    matchmaking::{
        CreatePreparingMatch, MatchVersions, MatchmakingService, ParticipantDeath,
        ParticipantRecord, ParticipantTimeout, StoredMatch,
    },
    ports::{
        AuthRepository, AuthRepositoryError, Clock, LoginCommand, LoginResult, MatchRepository,
        MatchRepositoryError, MatchWorldRuntime, MatchWorldRuntimeError, MatchWorldSpec, NewNonce,
        PreparedMatchWorld, RandomIdGenerator, RandomSeedGenerator, RepositoryFuture,
        RepositoryProbe, SessionRecord, SettlingTrigger, StoredNonce, TransitionOutcome,
        WarehouseSnapshot,
    },
};
use signinwithethereum::Message;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use uuid::Uuid;

#[derive(Clone)]
pub struct MemoryRepository {
    state: Arc<Mutex<MemoryState>>,
}

#[derive(Default)]
struct MemoryState {
    nonces: HashMap<[u8; 32], StoredNonce>,
    sessions: HashMap<[u8; 32], MemorySession>,
    wallets: HashMap<(i64, [u8; 20]), Uuid>,
    warehouse: HashMap<Uuid, WarehouseSnapshot>,
    nonce_prune_delay: Duration,
    nonce_prune_should_fail: bool,
    nonce_prune_calls: u64,
}

struct MemorySession {
    record: SessionRecord,
    last_seen_at: OffsetDateTime,
}

impl MemoryRepository {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            state: Arc::new(Mutex::new(MemoryState::default())),
        })
    }

    #[allow(dead_code)]
    pub fn seed_warehouse(&self, account_id: Uuid, snapshot: WarehouseSnapshot) {
        self.state
            .lock()
            .unwrap()
            .warehouse
            .insert(account_id, snapshot);
    }

    #[allow(dead_code)]
    pub fn account_for_wallet(&self, chain_id: i64, address: [u8; 20]) -> Option<Uuid> {
        self.state
            .lock()
            .unwrap()
            .wallets
            .get(&(chain_id, address))
            .copied()
    }

    #[allow(dead_code)]
    pub fn state_has_nonce_for_test(&self, nonce_hash: [u8; 32]) -> bool {
        self.state.lock().unwrap().nonces.contains_key(&nonce_hash)
    }

    #[allow(dead_code)]
    pub fn configure_nonce_pruning(&self, delay: Duration, should_fail: bool) {
        let mut state = self.state.lock().unwrap();
        state.nonce_prune_delay = delay;
        state.nonce_prune_should_fail = should_fail;
    }

    #[allow(dead_code)]
    pub fn nonce_prune_calls(&self) -> u64 {
        self.state.lock().unwrap().nonce_prune_calls
    }
}

impl RepositoryProbe for MemoryRepository {
    fn check(&self) -> RepositoryFuture<'_> {
        Box::pin(async { Ok(()) })
    }
}

#[async_trait]
impl AuthRepository for MemoryRepository {
    async fn insert_nonce(&self, nonce: NewNonce) -> Result<(), AuthRepositoryError> {
        self.state.lock().unwrap().nonces.insert(
            nonce.nonce_hash,
            StoredNonce {
                domain: nonce.domain,
                uri: nonce.uri,
                expires_at: nonce.expires_at,
                consumed_at: None,
            },
        );
        Ok(())
    }

    async fn find_nonce(
        &self,
        nonce_hash: [u8; 32],
    ) -> Result<Option<StoredNonce>, AuthRepositoryError> {
        Ok(self.state.lock().unwrap().nonces.get(&nonce_hash).cloned())
    }

    async fn prune_expired_nonces(
        &self,
        now: OffsetDateTime,
        limit: u32,
    ) -> Result<u64, AuthRepositoryError> {
        let (delay, should_fail) = {
            let mut state = self.state.lock().unwrap();
            state.nonce_prune_calls += 1;
            (state.nonce_prune_delay, state.nonce_prune_should_fail)
        };
        if !delay.is_zero() {
            actix_web::rt::time::sleep(delay).await;
        }
        if should_fail {
            return Err(AuthRepositoryError::Unavailable);
        }
        let mut state = self.state.lock().unwrap();
        let mut expired = state
            .nonces
            .iter()
            .filter(|(_, nonce)| nonce.expires_at <= now)
            .map(|(hash, nonce)| (*hash, nonce.expires_at))
            .collect::<Vec<_>>();
        expired.sort_unstable_by_key(|(hash, expires_at)| (*expires_at, *hash));
        let remove_count = expired.len().min(limit.min(512) as usize);
        for (hash, _) in expired.into_iter().take(remove_count) {
            state.nonces.remove(&hash);
        }
        Ok(remove_count as u64)
    }

    async fn complete_login(
        &self,
        command: LoginCommand,
    ) -> Result<LoginResult, AuthRepositoryError> {
        let mut state = self.state.lock().unwrap();
        let nonce = state
            .nonces
            .get_mut(&command.nonce_hash)
            .ok_or(AuthRepositoryError::NonceInvalid)?;
        if nonce.consumed_at.is_some()
            || nonce.expires_at <= command.now
            || nonce.domain != command.nonce_domain
            || nonce.uri != command.nonce_uri
        {
            return Err(AuthRepositoryError::NonceInvalid);
        }
        if command.authentication_expires_at <= command.now {
            return Err(AuthRepositoryError::AuthenticationExpired);
        }
        nonce.consumed_at = Some(command.now);
        let account_id = *state
            .wallets
            .entry((command.chain_id, command.address))
            .or_insert(command.proposed_account_id);
        let revoked_session_ids = state
            .sessions
            .values()
            .filter(|session| session.record.account_id == account_id)
            .map(|session| session.record.session_id)
            .collect();
        state
            .sessions
            .retain(|_, session| session.record.account_id != account_id);
        let record = SessionRecord {
            session_id: command.session_id,
            account_id,
            chain_id: command.chain_id,
            address: command.address,
            expires_at: command.session_expires_at,
            idle_expires_at: command.session_idle_expires_at,
        };
        state.sessions.insert(
            command.session_token_hash,
            MemorySession {
                record: record.clone(),
                last_seen_at: command.now,
            },
        );
        Ok(LoginResult {
            session: record,
            revoked_session_ids,
        })
    }

    async fn find_active_session(
        &self,
        token_hash: [u8; 32],
        now: OffsetDateTime,
    ) -> Result<Option<SessionRecord>, AuthRepositoryError> {
        let mut state = self.state.lock().unwrap();
        let Some(session) = state.sessions.get_mut(&token_hash) else {
            return Ok(None);
        };
        let effective_now = now.max(session.last_seen_at);
        if session.record.expires_at <= effective_now
            || session.record.idle_expires_at <= effective_now
        {
            return Ok(None);
        }
        let idle_window = session.record.idle_expires_at - session.last_seen_at;
        session.last_seen_at = effective_now;
        session.record.idle_expires_at =
            (effective_now + idle_window).min(session.record.expires_at);
        Ok(Some(session.record.clone()))
    }

    async fn inspect_active_session(
        &self,
        token_hash: [u8; 32],
        now: OffsetDateTime,
    ) -> Result<Option<SessionRecord>, AuthRepositoryError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .sessions
            .get(&token_hash)
            .filter(|session| {
                session.record.expires_at > now && session.record.idle_expires_at > now
            })
            .map(|session| session.record.clone()))
    }

    async fn revoke_session(
        &self,
        token_hash: [u8; 32],
        _now: OffsetDateTime,
    ) -> Result<Option<Uuid>, AuthRepositoryError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .sessions
            .remove(&token_hash)
            .map(|session| session.record.session_id))
    }

    async fn warehouse(&self, account_id: Uuid) -> Result<WarehouseSnapshot, AuthRepositoryError> {
        Ok(self
            .state
            .lock()
            .unwrap()
            .warehouse
            .get(&account_id)
            .cloned()
            .unwrap_or_default())
    }
}

#[derive(Clone, Copy)]
pub struct FixedClock {
    pub now: OffsetDateTime,
}

impl Clock for FixedClock {
    fn monotonic_now(&self) -> Duration {
        Duration::ZERO
    }

    fn utc_now(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(self.now.unix_timestamp() as u64)
    }
}

#[derive(Clone)]
pub struct FixedRandom {
    pub nonce: String,
    pub token: String,
}

impl AuthRandom for FixedRandom {
    fn nonce(&self) -> Result<String, AuthRandomError> {
        Ok(self.nonce.clone())
    }

    fn session_token(&self) -> Result<String, AuthRandomError> {
        Ok(self.token.clone())
    }
}

pub struct AcceptVerifier;

#[async_trait]
impl SignatureVerifier for AcceptVerifier {
    async fn verify(
        &self,
        _message: &Message,
        _signature: &[u8],
        _expected_nonce: &str,
        _now: OffsetDateTime,
    ) -> Result<(), SignatureVerificationError> {
        Ok(())
    }
}

#[allow(dead_code)]
pub struct EmptyMatchRepository;

impl RepositoryProbe for EmptyMatchRepository {
    fn check(&self) -> RepositoryFuture<'_> {
        Box::pin(async { Ok(()) })
    }
}

#[async_trait]
impl MatchRepository for EmptyMatchRepository {
    async fn create_preparing(
        &self,
        _command: CreatePreparingMatch,
    ) -> Result<StoredMatch, MatchRepositoryError> {
        Err(MatchRepositoryError::Unavailable)
    }

    async fn find_match(
        &self,
        _match_id: Uuid,
    ) -> Result<Option<StoredMatch>, MatchRepositoryError> {
        Ok(None)
    }

    async fn find_nonterminal_by_account(
        &self,
        _account_id: Uuid,
    ) -> Result<Option<StoredMatch>, MatchRepositoryError> {
        Ok(None)
    }

    async fn abort_unrecoverable_matches(
        &self,
        _reason: String,
        _at: OffsetDateTime,
    ) -> Result<u64, MatchRepositoryError> {
        Ok(0)
    }

    async fn activate(
        &self,
        _match_id: Uuid,
        _started_at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
        Err(MatchRepositoryError::Unavailable)
    }

    async fn abort(
        &self,
        _match_id: Uuid,
        _reason: String,
        _at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
        Err(MatchRepositoryError::Unavailable)
    }

    async fn mark_disconnected(
        &self,
        _match_id: Uuid,
        _account_id: Uuid,
        _at: OffsetDateTime,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError> {
        Err(MatchRepositoryError::Unavailable)
    }

    async fn reconnect(
        &self,
        _match_id: Uuid,
        _account_id: Uuid,
        _at: OffsetDateTime,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError> {
        Err(MatchRepositoryError::Unavailable)
    }

    async fn mark_dead(
        &self,
        _death: ParticipantDeath,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError> {
        Err(MatchRepositoryError::Unavailable)
    }

    async fn mark_timed_out(
        &self,
        _timeout: ParticipantTimeout,
        _at: OffsetDateTime,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError> {
        Err(MatchRepositoryError::Unavailable)
    }

    async fn open_extraction(
        &self,
        _match_id: Uuid,
        _at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
        Err(MatchRepositoryError::Unavailable)
    }

    async fn begin_settling(
        &self,
        _match_id: Uuid,
        _trigger: SettlingTrigger,
        _at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
        Err(MatchRepositoryError::Unavailable)
    }

    async fn finish(
        &self,
        _match_id: Uuid,
        _at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
        Err(MatchRepositoryError::Unavailable)
    }
}

struct EmptyWorldRuntime;

#[async_trait]
impl MatchWorldRuntime for EmptyWorldRuntime {
    async fn prepare_world(
        &self,
        _spec: MatchWorldSpec,
    ) -> Result<PreparedMatchWorld, MatchWorldRuntimeError> {
        Ok(PreparedMatchWorld {
            world_generation: "test-world-generation".to_owned(),
        })
    }

    async fn stop_world(
        &self,
        _match_id: Uuid,
        _world_name: &str,
    ) -> Result<bool, MatchWorldRuntimeError> {
        Ok(false)
    }

    async fn despawn_detached(
        &self,
        _world_name: &str,
        _account_id: Uuid,
    ) -> Result<bool, MatchWorldRuntimeError> {
        Ok(false)
    }

    async fn evict_participant(
        &self,
        _world_name: &str,
        _account_id: Uuid,
    ) -> Result<bool, MatchWorldRuntimeError> {
        Ok(false)
    }

    async fn request_timeout_elimination(
        &self,
        _world_name: &str,
        _account_id: Uuid,
    ) -> Result<bool, MatchWorldRuntimeError> {
        Ok(false)
    }
}

#[allow(dead_code)]
pub async fn empty_matchmaking(clock: Arc<dyn Clock>) -> Arc<MatchmakingService> {
    let service = MatchmakingService::start(
        Arc::new(EmptyMatchRepository),
        clock,
        Arc::new(RandomIdGenerator),
        Arc::new(RandomSeedGenerator),
        MatchVersions {
            generation: "generation-v1".to_owned(),
            gameplay: "gameplay-v1".to_owned(),
            config: "config-v1".to_owned(),
        },
    );
    service
        .bind_runtime(Arc::new(EmptyWorldRuntime))
        .await
        .unwrap();
    service
}

pub fn service_at(now: OffsetDateTime) -> (AuthService, Arc<MemoryRepository>) {
    let repository = MemoryRepository::new();
    let config = AuthConfig::local("127.0.0.1:5173", "http://127.0.0.1:5173");
    let clock: Arc<dyn Clock> = Arc::new(FixedClock { now });
    let auth = AuthService::new(
        repository.clone(),
        Arc::new(AcceptVerifier),
        clock,
        Arc::new(FixedRandom {
            nonce: "fixedNonce123456".to_owned(),
            token: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned(),
        }),
        config,
    );
    (auth, repository)
}

#[allow(dead_code)]
pub fn siwe_message(nonce: &str, address: &str, now: OffsetDateTime) -> String {
    let expiration = (now + time::Duration::minutes(4)).format(&Rfc3339).unwrap();
    format!(
        "127.0.0.1:5173 wants you to sign in with your Ethereum account:\n{address}\n\nSign in to Voxel Extraction.\n\nURI: http://127.0.0.1:5173\nVersion: 1\nChain ID: 1\nNonce: {nonce}\nIssued At: {}\nExpiration Time: {expiration}",
        now.format(&Rfc3339).unwrap()
    )
}
