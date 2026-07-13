use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{matchmaking::StoredMatch, ports::MatchRepositoryError};

use super::{
    rows::{MatchRow, ParticipantRow, MATCH_COLUMNS, PARTICIPANT_COLUMNS},
    unavailable,
};

pub(in crate::persistence::postgres) async fn find_match(
    pool: &PgPool,
    match_id: Uuid,
) -> Result<Option<StoredMatch>, MatchRepositoryError> {
    let mut transaction = read_snapshot(pool).await?;
    let query = format!("SELECT {MATCH_COLUMNS} FROM matches WHERE id = $1");
    let Some(row) = sqlx::query_as::<_, MatchRow>(&query)
        .bind(match_id)
        .fetch_optional(&mut *transaction)
        .await
        .map_err(unavailable)?
    else {
        transaction.commit().await.map_err(unavailable)?;
        return Ok(None);
    };
    let stored = load_match_in_transaction(&mut transaction, row).await?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(Some(stored))
}

pub(in crate::persistence::postgres) async fn find_nonterminal_by_account(
    pool: &PgPool,
    account_id: Uuid,
) -> Result<Option<StoredMatch>, MatchRepositoryError> {
    let mut transaction = read_snapshot(pool).await?;
    let match_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT participant.match_id \
         FROM match_participants AS participant \
         JOIN matches AS match ON match.id = participant.match_id \
         WHERE participant.account_id = $1 \
           AND participant.state IN (\
             'waiting', 'preparing', 'active', 'disconnected', 'settlement_pending'\
           ) \
           AND match.state NOT IN ('finished', 'aborted') \
         ORDER BY match.created_at DESC LIMIT 1",
    )
    .bind(account_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(unavailable)?;
    let Some(match_id) = match_id else {
        transaction.commit().await.map_err(unavailable)?;
        return Ok(None);
    };
    let query = format!("SELECT {MATCH_COLUMNS} FROM matches WHERE id = $1");
    let row = sqlx::query_as::<_, MatchRow>(&query)
        .bind(match_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(unavailable)?;
    let stored = load_match_in_transaction(&mut transaction, row).await?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(Some(stored))
}

pub(in crate::persistence::postgres) async fn lock_match(
    transaction: &mut Transaction<'_, Postgres>,
    match_id: Uuid,
) -> Result<MatchRow, MatchRepositoryError> {
    let query = format!("SELECT {MATCH_COLUMNS} FROM matches WHERE id = $1 FOR UPDATE");
    sqlx::query_as::<_, MatchRow>(&query)
        .bind(match_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(unavailable)?
        .ok_or(MatchRepositoryError::Conflict)
}

pub(in crate::persistence::postgres) async fn load_match_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    row: MatchRow,
) -> Result<StoredMatch, MatchRepositoryError> {
    let query = format!(
        "SELECT {PARTICIPANT_COLUMNS} FROM match_participants \
         WHERE match_id = $1 ORDER BY seat_id"
    );
    let participants = sqlx::query_as::<_, ParticipantRow>(&query)
        .bind(row.id)
        .fetch_all(&mut **transaction)
        .await
        .map_err(unavailable)?
        .into_iter()
        .map(ParticipantRow::into_record)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(StoredMatch {
        record: row.into_record()?,
        participants,
    })
}

async fn read_snapshot(pool: &PgPool) -> Result<Transaction<'_, Postgres>, MatchRepositoryError> {
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    // 两张表必须来自同一快照，否则审计结果可能跨越一次生命周期提交。
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
    Ok(transaction)
}

pub(in crate::persistence::postgres) async fn lock_participant(
    transaction: &mut Transaction<'_, Postgres>,
    match_id: Uuid,
    account_id: Uuid,
) -> Result<ParticipantRow, MatchRepositoryError> {
    let query = format!(
        "SELECT {PARTICIPANT_COLUMNS} FROM match_participants \
         WHERE match_id = $1 AND account_id = $2 FOR UPDATE"
    );
    sqlx::query_as::<_, ParticipantRow>(&query)
        .bind(match_id)
        .bind(account_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(unavailable)?
        .ok_or(MatchRepositoryError::Conflict)
}
