use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::ports::{AuthRepositoryError, SessionRecord};

pub(super) async fn find_active_session(
    pool: &PgPool,
    token_hash: [u8; 32],
    now: OffsetDateTime,
) -> Result<Option<SessionRecord>, AuthRepositoryError> {
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    let row = sqlx::query_as::<_, LockedSessionRow>(
        "SELECT id, account_id, chain_id, address, expires_at, idle_expires_at, \
                revoked_at, last_seen_at \
         FROM auth_sessions WHERE token_hash = $1 FOR UPDATE",
    )
    .bind(token_hash.as_slice())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(unavailable)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let database_now =
        sqlx::query_scalar::<_, OffsetDateTime>("SELECT GREATEST($1, clock_timestamp())")
            .bind(now)
            .fetch_one(&mut *transaction)
            .await
            .map_err(unavailable)?;
    let effective_now = database_now.max(row.last_seen_at);
    if row.revoked_at.is_some()
        || row.expires_at <= effective_now
        || row.idle_expires_at <= effective_now
    {
        return Ok(None);
    }

    let idle_expires_at =
        (row.idle_expires_at + (effective_now - row.last_seen_at)).min(row.expires_at);
    sqlx::query("UPDATE auth_sessions SET last_seen_at = $2, idle_expires_at = $3 WHERE id = $1")
        .bind(row.id)
        .bind(effective_now)
        .bind(idle_expires_at)
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
    transaction.commit().await.map_err(unavailable)?;

    SessionRecord::try_from(SessionRow {
        id: row.id,
        account_id: row.account_id,
        chain_id: row.chain_id,
        address: row.address,
        expires_at: row.expires_at,
        idle_expires_at,
    })
    .map(Some)
}

pub(super) async fn inspect_active_session(
    pool: &PgPool,
    token_hash: [u8; 32],
    now: OffsetDateTime,
) -> Result<Option<SessionRecord>, AuthRepositoryError> {
    let row = sqlx::query_as::<_, SessionRow>(
        "SELECT id, account_id, chain_id, address, expires_at, idle_expires_at \
         FROM auth_sessions \
         WHERE token_hash = $1 AND revoked_at IS NULL \
           AND expires_at > GREATEST($2, clock_timestamp()) \
           AND idle_expires_at > GREATEST($2, clock_timestamp())",
    )
    .bind(token_hash.as_slice())
    .bind(now)
    .fetch_optional(pool)
    .await
    .map_err(unavailable)?;
    row.map(SessionRecord::try_from).transpose()
}

pub(super) async fn revoke_session(
    pool: &PgPool,
    token_hash: [u8; 32],
    now: OffsetDateTime,
) -> Result<Option<Uuid>, AuthRepositoryError> {
    sqlx::query_scalar::<_, Uuid>(
        "UPDATE auth_sessions \
         SET revoked_at = GREATEST($2, clock_timestamp(), created_at) \
         WHERE token_hash = $1 AND revoked_at IS NULL RETURNING id",
    )
    .bind(token_hash.as_slice())
    .bind(now)
    .fetch_optional(pool)
    .await
    .map_err(unavailable)
}

#[derive(sqlx::FromRow)]
struct SessionRow {
    id: Uuid,
    account_id: Uuid,
    chain_id: i64,
    address: Vec<u8>,
    expires_at: OffsetDateTime,
    idle_expires_at: OffsetDateTime,
}

#[derive(sqlx::FromRow)]
struct LockedSessionRow {
    id: Uuid,
    account_id: Uuid,
    chain_id: i64,
    address: Vec<u8>,
    expires_at: OffsetDateTime,
    idle_expires_at: OffsetDateTime,
    revoked_at: Option<OffsetDateTime>,
    last_seen_at: OffsetDateTime,
}

impl TryFrom<SessionRow> for SessionRecord {
    type Error = AuthRepositoryError;

    fn try_from(value: SessionRow) -> Result<Self, Self::Error> {
        let address = value
            .address
            .try_into()
            .map_err(|_| AuthRepositoryError::Unavailable)?;
        Ok(Self {
            session_id: value.id,
            account_id: value.account_id,
            chain_id: value.chain_id,
            address,
            expires_at: value.expires_at,
            idle_expires_at: value.idle_expires_at,
        })
    }
}

fn unavailable(_: sqlx::Error) -> AuthRepositoryError {
    AuthRepositoryError::Unavailable
}
