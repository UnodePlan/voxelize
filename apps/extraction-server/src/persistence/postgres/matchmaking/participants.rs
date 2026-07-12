use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    matchmaking::{MatchState, ParticipantRecord, ParticipantState, RECONNECT_WINDOW},
    ports::{MatchRepositoryError, TransitionOutcome},
};

use super::{
    classify_write_error,
    read::{lock_match, lock_participant},
    unavailable,
};

pub(in crate::persistence::postgres) async fn mark_disconnected(
    pool: &PgPool,
    match_id: Uuid,
    account_id: Uuid,
    at: OffsetDateTime,
) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError> {
    let reconnect_deadline = at
        .checked_add(RECONNECT_WINDOW)
        .ok_or(MatchRepositoryError::Conflict)?;
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    let match_state = lock_match(&mut transaction, match_id)
        .await?
        .parsed_state()?;
    let mut participant = lock_participant(&mut transaction, match_id, account_id).await?;
    let participant_state = participant.parsed_state()?;
    if participant_state == ParticipantState::Disconnected {
        let record = participant.into_record()?;
        transaction.commit().await.map_err(unavailable)?;
        return Ok(TransitionOutcome::AlreadyApplied(record));
    }
    require_live_match(match_state)?;
    participant_state
        .transition_to(ParticipantState::Disconnected)
        .map_err(|_| MatchRepositoryError::Conflict)?;

    let result = sqlx::query(
        "UPDATE match_participants SET state = 'disconnected', reconnect_deadline = $3 \
         WHERE match_id = $1 AND account_id = $2 AND state = 'active'",
    )
    .bind(match_id)
    .bind(account_id)
    .bind(reconnect_deadline)
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;
    if result.rows_affected() != 1 {
        return Err(MatchRepositoryError::Conflict);
    }
    participant.state = ParticipantState::Disconnected.as_str().to_owned();
    participant.reconnect_deadline = Some(reconnect_deadline);
    let record = participant.into_record()?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(TransitionOutcome::Applied(record))
}

pub(in crate::persistence::postgres) async fn reconnect(
    pool: &PgPool,
    match_id: Uuid,
    account_id: Uuid,
    at: OffsetDateTime,
) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError> {
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    let match_state = lock_match(&mut transaction, match_id)
        .await?
        .parsed_state()?;
    let mut participant = lock_participant(&mut transaction, match_id, account_id).await?;
    let participant_state = participant.parsed_state()?;
    if participant_state == ParticipantState::Active && participant.reconnect_deadline.is_none() {
        require_live_match(match_state)?;
        let record = participant.into_record()?;
        transaction.commit().await.map_err(unavailable)?;
        return Ok(TransitionOutcome::AlreadyApplied(record));
    }
    require_live_match(match_state)?;
    participant_state
        .transition_to(ParticipantState::Active)
        .map_err(|_| MatchRepositoryError::Conflict)?;
    // 60 秒边界归超时路径所有，避免重连与超时在同一时刻都满足条件。
    if participant
        .reconnect_deadline
        .is_none_or(|deadline| at >= deadline)
    {
        return Err(MatchRepositoryError::Conflict);
    }

    let result = sqlx::query(
        "UPDATE match_participants SET state = 'active', reconnect_deadline = NULL \
         WHERE match_id = $1 AND account_id = $2 AND state = 'disconnected' \
           AND reconnect_deadline > $3",
    )
    .bind(match_id)
    .bind(account_id)
    .bind(at)
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;
    if result.rows_affected() != 1 {
        return Err(MatchRepositoryError::Conflict);
    }
    participant.state = ParticipantState::Active.as_str().to_owned();
    participant.reconnect_deadline = None;
    let record = participant.into_record()?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(TransitionOutcome::Applied(record))
}

pub(in crate::persistence::postgres) async fn time_out(
    pool: &PgPool,
    match_id: Uuid,
    account_id: Uuid,
    at: OffsetDateTime,
) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError> {
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    let match_state = lock_match(&mut transaction, match_id)
        .await?
        .parsed_state()?;
    let mut participant = lock_participant(&mut transaction, match_id, account_id).await?;
    let participant_state = participant.parsed_state()?;
    if participant_state == ParticipantState::TimedOut {
        let record = participant.into_record()?;
        transaction.commit().await.map_err(unavailable)?;
        return Ok(TransitionOutcome::AlreadyApplied(record));
    }
    require_live_match(match_state)?;
    participant_state
        .transition_to(ParticipantState::TimedOut)
        .map_err(|_| MatchRepositoryError::Conflict)?;
    if participant
        .reconnect_deadline
        .is_none_or(|deadline| at < deadline)
    {
        return Err(MatchRepositoryError::Conflict);
    }

    let result = sqlx::query(
        "UPDATE match_participants SET state = 'timed_out', reconnect_deadline = NULL \
         WHERE match_id = $1 AND account_id = $2 AND state = 'disconnected' \
           AND reconnect_deadline <= $3",
    )
    .bind(match_id)
    .bind(account_id)
    .bind(at)
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;
    if result.rows_affected() != 1 {
        return Err(MatchRepositoryError::Conflict);
    }
    participant.state = ParticipantState::TimedOut.as_str().to_owned();
    participant.reconnect_deadline = None;
    let record = participant.into_record()?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(TransitionOutcome::Applied(record))
}

fn require_live_match(state: MatchState) -> Result<(), MatchRepositoryError> {
    matches!(state, MatchState::Active | MatchState::ExtractionOpen)
        .then_some(())
        .ok_or(MatchRepositoryError::Conflict)
}
