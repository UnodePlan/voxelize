use async_trait::async_trait;
use time::OffsetDateTime;
use uuid::Uuid;

use super::RepositoryProbe;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NewNonce {
    pub id: Uuid,
    pub nonce_hash: [u8; 32],
    pub domain: String,
    pub uri: String,
    pub created_at: OffsetDateTime,
    pub expires_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredNonce {
    pub domain: String,
    pub uri: String,
    pub expires_at: OffsetDateTime,
    pub consumed_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoginCommand {
    pub nonce_hash: [u8; 32],
    pub nonce_domain: String,
    pub nonce_uri: String,
    pub proposed_account_id: Uuid,
    pub session_id: Uuid,
    pub session_token_hash: [u8; 32],
    pub chain_id: i64,
    pub address: [u8; 20],
    pub now: OffsetDateTime,
    pub authentication_expires_at: OffsetDateTime,
    pub session_expires_at: OffsetDateTime,
    pub session_idle_expires_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionRecord {
    pub session_id: Uuid,
    pub account_id: Uuid,
    pub chain_id: i64,
    pub address: [u8; 20],
    pub expires_at: OffsetDateTime,
    pub idle_expires_at: OffsetDateTime,
}

impl SessionRecord {
    pub fn valid_until(&self) -> OffsetDateTime {
        self.expires_at.min(self.idle_expires_at)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LoginResult {
    pub session: SessionRecord,
    pub revoked_session_ids: Vec<Uuid>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WarehouseSnapshot {
    pub dirt: i64,
    pub gold: i64,
    pub diamond: i64,
    pub stats: WarehouseStats,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct WarehouseStats {
    pub total_resources_extracted: i64,
    pub total_extraction_value: i64,
    pub successful_extractions: i64,
    pub highest_single_match_value: i64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthRepositoryError {
    NonceInvalid,
    AuthenticationExpired,
    Unavailable,
}

#[async_trait]
pub trait AuthRepository: RepositoryProbe + Send + Sync {
    async fn insert_nonce(&self, nonce: NewNonce) -> Result<(), AuthRepositoryError>;

    async fn find_nonce(
        &self,
        nonce_hash: [u8; 32],
    ) -> Result<Option<StoredNonce>, AuthRepositoryError>;

    async fn prune_expired_nonces(
        &self,
        now: OffsetDateTime,
        limit: u32,
    ) -> Result<u64, AuthRepositoryError>;

    async fn complete_login(
        &self,
        command: LoginCommand,
    ) -> Result<LoginResult, AuthRepositoryError>;

    async fn find_active_session(
        &self,
        token_hash: [u8; 32],
        now: OffsetDateTime,
    ) -> Result<Option<SessionRecord>, AuthRepositoryError>;

    async fn inspect_active_session(
        &self,
        token_hash: [u8; 32],
        now: OffsetDateTime,
    ) -> Result<Option<SessionRecord>, AuthRepositoryError>;

    async fn revoke_session(
        &self,
        token_hash: [u8; 32],
        now: OffsetDateTime,
    ) -> Result<Option<Uuid>, AuthRepositoryError>;

    async fn warehouse(&self, account_id: Uuid) -> Result<WarehouseSnapshot, AuthRepositoryError>;
}
