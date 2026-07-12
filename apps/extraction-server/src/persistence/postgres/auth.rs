use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::ports::{
    AuthRepositoryError, LoginCommand, LoginResult, NewNonce, SessionRecord, StoredNonce,
};

const MAX_NONCE_PRUNE_BATCH: u32 = 512;

pub(super) async fn insert_nonce(
    pool: &PgPool,
    nonce: NewNonce,
) -> Result<(), AuthRepositoryError> {
    sqlx::query(
        "INSERT INTO auth_nonces \
         (id, nonce_hash, domain, uri, expires_at, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(nonce.id)
    .bind(nonce.nonce_hash.as_slice())
    .bind(nonce.domain)
    .bind(nonce.uri)
    .bind(nonce.expires_at)
    .bind(nonce.created_at)
    .execute(pool)
    .await
    .map(|_| ())
    .map_err(unavailable)
}

pub(super) async fn find_nonce(
    pool: &PgPool,
    nonce_hash: [u8; 32],
) -> Result<Option<StoredNonce>, AuthRepositoryError> {
    let row = sqlx::query_as::<_, NonceRow>(
        "SELECT domain, uri, expires_at, consumed_at \
         FROM auth_nonces WHERE nonce_hash = $1",
    )
    .bind(nonce_hash.as_slice())
    .fetch_optional(pool)
    .await
    .map_err(unavailable)?;
    Ok(row.map(Into::into))
}

pub(super) async fn prune_expired_nonces(
    pool: &PgPool,
    now: OffsetDateTime,
    limit: u32,
) -> Result<u64, AuthRepositoryError> {
    if limit == 0 {
        return Ok(0);
    }
    let limit = i64::from(limit.min(MAX_NONCE_PRUNE_BATCH));
    let result = sqlx::query(
        "WITH expired AS (\
             SELECT id FROM auth_nonces \
             WHERE expires_at <= $1 \
             ORDER BY expires_at, id LIMIT $2\
         ) \
         DELETE FROM auth_nonces AS nonce USING expired \
         WHERE nonce.id = expired.id",
    )
    .bind(now)
    .bind(limit)
    .execute(pool)
    .await
    .map_err(unavailable)?;
    Ok(result.rows_affected())
}

pub(super) async fn complete_login(
    pool: &PgPool,
    mut command: LoginCommand,
) -> Result<LoginResult, AuthRepositoryError> {
    let session_ttl = command.session_expires_at - command.now;
    let session_idle_ttl = command.session_idle_expires_at - command.now;
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    let nonce = sqlx::query_as::<_, NonceRow>(
        "SELECT domain, uri, expires_at, consumed_at \
         FROM auth_nonces WHERE nonce_hash = $1 FOR UPDATE",
    )
    .bind(command.nonce_hash.as_slice())
    .fetch_optional(&mut *transaction)
    .await
    .map_err(unavailable)?;
    let nonce_checked_at =
        sqlx::query_scalar::<_, OffsetDateTime>("SELECT GREATEST($1, clock_timestamp())")
            .bind(command.now)
            .fetch_one(&mut *transaction)
            .await
            .map_err(unavailable)?;
    let Some(nonce) = nonce else {
        return Err(AuthRepositoryError::NonceInvalid);
    };
    if nonce.consumed_at.is_some()
        || nonce.expires_at <= nonce_checked_at
        || nonce.domain != command.nonce_domain
        || nonce.uri != command.nonce_uri
    {
        return Err(AuthRepositoryError::NonceInvalid);
    }
    if command.authentication_expires_at <= nonce_checked_at {
        return Err(AuthRepositoryError::AuthenticationExpired);
    }
    command.now = nonce_checked_at;

    let account_id = resolve_account(&mut transaction, &command).await?;
    lock_account(&mut transaction, account_id).await?;
    lock_active_sessions(&mut transaction, account_id).await?;
    let database_now =
        sqlx::query_scalar::<_, OffsetDateTime>("SELECT GREATEST($1, clock_timestamp())")
            .bind(command.now)
            .fetch_one(&mut *transaction)
            .await
            .map_err(unavailable)?;
    if nonce.expires_at <= database_now {
        return Err(AuthRepositoryError::NonceInvalid);
    }
    if command.authentication_expires_at <= database_now {
        return Err(AuthRepositoryError::AuthenticationExpired);
    }
    sqlx::query(
        "UPDATE auth_nonces SET consumed_at = GREATEST($2, created_at) \
         WHERE nonce_hash = $1 AND consumed_at IS NULL",
    )
    .bind(command.nonce_hash.as_slice())
    .bind(database_now)
    .execute(&mut *transaction)
    .await
    .map_err(unavailable)?;

    command.now = database_now;
    command.session_expires_at = database_now + session_ttl;
    command.session_idle_expires_at = database_now + session_idle_ttl;
    let revoked_session_ids = sqlx::query_scalar::<_, Uuid>(
        "UPDATE auth_sessions \
         SET revoked_at = COALESCE(\
             revoked_at, GREATEST($2, clock_timestamp(), created_at)\
         ) \
         WHERE account_id = $1 AND revoked_at IS NULL RETURNING id",
    )
    .bind(account_id)
    .bind(command.now)
    .fetch_all(&mut *transaction)
    .await
    .map_err(unavailable)?;

    sqlx::query(
        "INSERT INTO auth_sessions \
         (id, token_hash, account_id, chain_id, address, expires_at, \
          idle_expires_at, created_at, last_seen_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)",
    )
    .bind(command.session_id)
    .bind(command.session_token_hash.as_slice())
    .bind(account_id)
    .bind(command.chain_id)
    .bind(command.address.as_slice())
    .bind(command.session_expires_at)
    .bind(command.session_idle_expires_at)
    .bind(command.now)
    .execute(&mut *transaction)
    .await
    .map_err(unavailable)?;
    transaction.commit().await.map_err(unavailable)?;

    Ok(LoginResult {
        session: SessionRecord {
            session_id: command.session_id,
            account_id,
            chain_id: command.chain_id,
            address: command.address,
            expires_at: command.session_expires_at,
            idle_expires_at: command.session_idle_expires_at,
        },
        revoked_session_ids,
    })
}

async fn resolve_account(
    transaction: &mut Transaction<'_, Postgres>,
    command: &LoginCommand,
) -> Result<Uuid, AuthRepositoryError> {
    if let Some(account_id) = find_account(transaction, command).await? {
        return Ok(account_id);
    }

    // 新钱包注册很少发生，表级写锁避免并发创建同一钱包时留下孤立账号。
    sqlx::query("LOCK TABLE wallet_credentials IN SHARE ROW EXCLUSIVE MODE")
        .execute(&mut **transaction)
        .await
        .map_err(unavailable)?;
    if let Some(account_id) = find_account(transaction, command).await? {
        return Ok(account_id);
    }

    sqlx::query("INSERT INTO accounts (id, created_at, updated_at) VALUES ($1, $2, $2)")
        .bind(command.proposed_account_id)
        .bind(command.now)
        .execute(&mut **transaction)
        .await
        .map_err(unavailable)?;
    sqlx::query(
        "INSERT INTO wallet_credentials (account_id, chain_id, address, created_at) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(command.proposed_account_id)
    .bind(command.chain_id)
    .bind(command.address.as_slice())
    .bind(command.now)
    .execute(&mut **transaction)
    .await
    .map_err(unavailable)?;
    Ok(command.proposed_account_id)
}

async fn lock_account(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
) -> Result<(), AuthRepositoryError> {
    // 同钱包的不同 nonce 可以同时通过验签；账号行锁保证会话替换按提交顺序串行。
    sqlx::query_scalar::<_, Uuid>("SELECT id FROM accounts WHERE id = $1 FOR UPDATE")
        .bind(account_id)
        .fetch_one(&mut **transaction)
        .await
        .map(|_| ())
        .map_err(unavailable)
}

async fn lock_active_sessions(
    transaction: &mut Transaction<'_, Postgres>,
    account_id: Uuid,
) -> Result<(), AuthRepositoryError> {
    // touch/logout 不锁账号；最终取时前先收齐会话行锁，避免等待期间跨过登录截止时间。
    sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM auth_sessions \
         WHERE account_id = $1 AND revoked_at IS NULL FOR UPDATE",
    )
    .bind(account_id)
    .fetch_all(&mut **transaction)
    .await
    .map(|_| ())
    .map_err(unavailable)
}

async fn find_account(
    transaction: &mut Transaction<'_, Postgres>,
    command: &LoginCommand,
) -> Result<Option<Uuid>, AuthRepositoryError> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT account_id FROM wallet_credentials WHERE chain_id = $1 AND address = $2",
    )
    .bind(command.chain_id)
    .bind(command.address.as_slice())
    .fetch_optional(&mut **transaction)
    .await
    .map_err(unavailable)
}

#[derive(sqlx::FromRow)]
struct NonceRow {
    domain: String,
    uri: String,
    expires_at: OffsetDateTime,
    consumed_at: Option<OffsetDateTime>,
}

impl From<NonceRow> for StoredNonce {
    fn from(value: NonceRow) -> Self {
        Self {
            domain: value.domain,
            uri: value.uri,
            expires_at: value.expires_at,
            consumed_at: value.consumed_at,
        }
    }
}

fn unavailable(_: sqlx::Error) -> AuthRepositoryError {
    AuthRepositoryError::Unavailable
}
