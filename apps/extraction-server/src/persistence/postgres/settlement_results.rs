use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

use crate::{
    matchmaking::{MatchResultRecord, ParticipantState},
    ports::SettlementRepositoryError,
};

use super::{
    matchmaking::rows::{MatchRow, ParticipantRow, MATCH_COLUMNS, PARTICIPANT_COLUMNS},
    settlement_read::{find_in_transaction, read_snapshot},
};

pub(super) async fn find_match_result(
    pool: &PgPool,
    match_id: Uuid,
    account_id: Uuid,
) -> Result<Option<MatchResultRecord>, SettlementRepositoryError> {
    let mut transaction = read_snapshot(pool).await?;
    let result = find_match_result_in_transaction(&mut transaction, match_id, account_id).await?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(result)
}

async fn find_match_result_in_transaction(
    transaction: &mut Transaction<'_, Postgres>,
    match_id: Uuid,
    account_id: Uuid,
) -> Result<Option<MatchResultRecord>, SettlementRepositoryError> {
    let match_query = format!("SELECT {MATCH_COLUMNS} FROM matches WHERE id = $1");
    let Some(match_row) = sqlx::query_as::<_, MatchRow>(&match_query)
        .bind(match_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(unavailable)?
    else {
        return Ok(None);
    };
    let participant_query = format!(
        "SELECT {PARTICIPANT_COLUMNS} FROM match_participants \
         WHERE match_id = $1 AND account_id = $2"
    );
    let Some(participant_row) = sqlx::query_as::<_, ParticipantRow>(&participant_query)
        .bind(match_id)
        .bind(account_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(unavailable)?
    else {
        return Ok(None);
    };
    let killer_public_player_id = match participant_row.killed_by_account_id {
        Some(killer_account_id) => sqlx::query_scalar::<_, Uuid>(
            "SELECT public_player_id FROM match_participants \
             WHERE match_id = $1 AND account_id = $2",
        )
        .bind(match_id)
        .bind(killer_account_id)
        .fetch_optional(&mut **transaction)
        .await
        .map_err(unavailable)?,
        None => None,
    };
    let settlement = find_in_transaction(transaction, match_id, account_id, false).await?;
    let match_record = match_row.into_record().map_err(map_match_error)?;
    let participant = participant_row.into_record().map_err(map_match_error)?;
    if participant.state == ParticipantState::Extracted && settlement.is_none() {
        return Err(SettlementRepositoryError::Invariant);
    }
    if settlement.is_some() && participant.state != ParticipantState::Extracted {
        return Err(SettlementRepositoryError::Invariant);
    }
    let snapshot_now =
        sqlx::query_scalar::<_, time::OffsetDateTime>("SELECT transaction_timestamp()")
            .fetch_one(&mut **transaction)
            .await
            .map_err(unavailable)?;
    // 宽限期后不再补写；仅把无结算的待定状态投影为异常失败，数据库仍保留审计原状。
    let participant_state = if participant.state == ParticipantState::SettlementPending
        && settlement.is_none()
        && match_record
            .settlement_grace_deadline
            .is_some_and(|deadline| snapshot_now > deadline)
    {
        ParticipantState::Aborted
    } else {
        participant.state
    };
    let result = MatchResultRecord {
        match_id,
        match_state: match_record.state,
        participant_state,
        public_player_id: participant.public_player_id,
        terminal_cause: participant.terminal_cause,
        killer_public_player_id,
        terminal_at: participant.terminal_at,
        survived_ms: participant.survived_ms,
        stats: participant.stats,
        settlement,
        abort_reason: match_record.abort_reason,
    };
    Ok(Some(result))
}

pub(super) async fn find_latest_match_result(
    pool: &PgPool,
    account_id: Uuid,
) -> Result<Option<MatchResultRecord>, SettlementRepositoryError> {
    let mut transaction = read_snapshot(pool).await?;
    let match_id = sqlx::query_scalar::<_, Uuid>(
        "SELECT participant.match_id FROM match_participants AS participant \
         JOIN matches AS match_record ON match_record.id = participant.match_id \
         WHERE participant.account_id = $1 \
         ORDER BY match_record.created_at DESC, participant.match_id DESC LIMIT 1",
    )
    .bind(account_id)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(unavailable)?;
    let result = match match_id {
        Some(match_id) => {
            find_match_result_in_transaction(&mut transaction, match_id, account_id).await?
        }
        None => None,
    };
    transaction.commit().await.map_err(unavailable)?;
    Ok(result)
}

fn map_match_error(_: crate::ports::MatchRepositoryError) -> SettlementRepositoryError {
    SettlementRepositoryError::Unavailable
}

fn unavailable(_: sqlx::Error) -> SettlementRepositoryError {
    SettlementRepositoryError::Unavailable
}
