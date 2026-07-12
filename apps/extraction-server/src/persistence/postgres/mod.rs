mod auth;
mod session;
mod warehouse;

use std::time::Duration;

use async_trait::async_trait;
use sqlx::{migrate::Migrator, postgres::PgPoolOptions, PgPool};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::ports::{
    AuthRepository, AuthRepositoryError, LoginCommand, LoginResult, NewNonce, RepositoryError,
    RepositoryFuture, RepositoryProbe, SessionRecord, StoredNonce, WarehouseSnapshot,
};

static MIGRATOR: Migrator = sqlx::migrate!("./migrations");

#[derive(Clone, Debug)]
pub struct PgRepository {
    pool: PgPool,
}

impl PgRepository {
    pub async fn connect(database_url: &str) -> Result<Self, sqlx::Error> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .acquire_timeout(Duration::from_secs(3))
            .connect(database_url)
            .await?;
        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
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
