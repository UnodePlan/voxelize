use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::ports::MatchRepositoryError;

use super::{classify_write_error, unavailable};

pub(in crate::persistence::postgres) async fn abort_unrecoverable_matches(
    pool: &PgPool,
    reason: String,
    at: OffsetDateTime,
) -> Result<u64, MatchRepositoryError> {
    if reason.trim().is_empty() {
        return Err(MatchRepositoryError::Conflict);
    }

    let mut transaction = pool.begin().await.map_err(unavailable)?;
    sqlx::query("SET LOCAL lock_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
    sqlx::query("SET LOCAL statement_timeout = '15s'")
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
    // 先按固定顺序锁完比赛，再更新参与者，保持与单局生命周期事务相同的锁层级。
    let match_ids = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM matches WHERE state NOT IN ('finished', 'aborted') \
         ORDER BY id FOR UPDATE",
    )
    .fetch_all(&mut *transaction)
    .await
    .map_err(unavailable)?;
    if match_ids.is_empty() {
        transaction.commit().await.map_err(unavailable)?;
        return Ok(0);
    }

    sqlx::query(
        "UPDATE match_participants SET state = 'aborted', reconnect_deadline = NULL \
         WHERE match_id = ANY($1) AND state IN (\
           'waiting', 'preparing', 'active', 'disconnected', 'settlement_pending'\
         )",
    )
    .bind(&match_ids)
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;

    let updated = sqlx::query(
        "UPDATE matches SET state = 'aborted', finished_at = GREATEST(created_at, $2), \
         abort_reason = $3 WHERE id = ANY($1) AND state NOT IN ('finished', 'aborted')",
    )
    .bind(&match_ids)
    .bind(at)
    .bind(&reason)
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;
    if updated.rows_affected() != match_ids.len() as u64 {
        return Err(MatchRepositoryError::Conflict);
    }

    transaction.commit().await.map_err(unavailable)?;
    Ok(updated.rows_affected())
}
