use sqlx::{types::Json, PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    matchmaking::{ExtractionQualification, MatchState, ParticipantRecord, ParticipantState},
    ports::{SettlementRepositoryError, TransitionOutcome},
};

use super::{
    matchmaking::read::{lock_match, lock_participant},
    settlement_assets::classify_write_error,
    settlement_read::find_in_transaction,
};

pub(super) async fn mark_pending(
    pool: &PgPool,
    qualification: ExtractionQualification,
) -> Result<TransitionOutcome<ParticipantRecord>, SettlementRepositoryError> {
    if !qualification.is_valid() {
        return Err(SettlementRepositoryError::Conflict);
    }
    let mut transaction = begin_write_transaction(pool).await?;
    let match_row = lock_match(&mut transaction, qualification.match_id)
        .await
        .map_err(map_match_error)?;
    let mut participant = lock_participant(
        &mut transaction,
        qualification.match_id,
        qualification.account_id,
    )
    .await
    .map_err(map_match_error)?;
    validate_qualification(&match_row, &qualification)?;
    let state = participant.parsed_state().map_err(map_match_error)?;
    if matches!(
        state,
        ParticipantState::SettlementPending | ParticipantState::Extracted
    ) {
        if participant.settlement_qualified_at != Some(qualification.qualified_at)
            || participant.stats() != qualification.stats
        {
            return Err(SettlementRepositoryError::Conflict);
        }
        let record = participant.into_record().map_err(map_match_error)?;
        transaction.commit().await.map_err(unavailable)?;
        return Ok(TransitionOutcome::AlreadyApplied(record));
    }
    if state != ParticipantState::Active {
        return Err(SettlementRepositoryError::Conflict);
    }
    let database_now = sqlx::query_scalar::<_, OffsetDateTime>("SELECT transaction_timestamp()")
        .fetch_one(&mut *transaction)
        .await
        .map_err(unavailable)?;
    if match_row
        .settlement_grace_deadline
        .is_none_or(|deadline| database_now > deadline)
    {
        return Err(SettlementRepositoryError::WindowClosed);
    }
    let result = sqlx::query(
        "UPDATE match_participants SET state = 'settlement_pending', \
         settlement_qualified_at = $3, reconnect_deadline = NULL, \
         mined_counts = $4, pickup_counts = $5, lost_counts = $6 \
         WHERE match_id = $1 AND account_id = $2 AND state = 'active'",
    )
    .bind(qualification.match_id)
    .bind(qualification.account_id)
    .bind(qualification.qualified_at)
    .bind(Json(qualification.stats.mined))
    .bind(Json(qualification.stats.picked_up))
    .bind(Json(qualification.stats.lost))
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;
    if result.rows_affected() != 1 {
        return Err(SettlementRepositoryError::Conflict);
    }
    participant.state = ParticipantState::SettlementPending.as_str().to_owned();
    participant.settlement_qualified_at = Some(qualification.qualified_at);
    participant.reconnect_deadline = None;
    participant.mined_counts = Json(qualification.stats.mined);
    participant.pickup_counts = Json(qualification.stats.picked_up);
    participant.lost_counts = Json(qualification.stats.lost);
    let record = participant.into_record().map_err(map_match_error)?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(TransitionOutcome::Applied(record))
}

pub(super) async fn abort_pending(
    pool: &PgPool,
    match_id: Uuid,
    account_id: Uuid,
    _at: OffsetDateTime,
) -> Result<TransitionOutcome<ParticipantRecord>, SettlementRepositoryError> {
    let mut transaction = begin_write_transaction(pool).await?;
    let match_row = lock_match(&mut transaction, match_id)
        .await
        .map_err(map_match_error)?;
    let mut participant = lock_participant(&mut transaction, match_id, account_id)
        .await
        .map_err(map_match_error)?;
    let state = participant.parsed_state().map_err(map_match_error)?;
    if state == ParticipantState::Aborted {
        let record = participant.into_record().map_err(map_match_error)?;
        transaction.commit().await.map_err(unavailable)?;
        return Ok(TransitionOutcome::AlreadyApplied(record));
    }
    if state != ParticipantState::SettlementPending
        || find_in_transaction(&mut transaction, match_id, account_id, true)
            .await?
            .is_some()
    {
        return Err(SettlementRepositoryError::Conflict);
    }
    let database_now = sqlx::query_scalar::<_, OffsetDateTime>("SELECT transaction_timestamp()")
        .fetch_one(&mut *transaction)
        .await
        .map_err(unavailable)?;
    if match_row
        .settlement_grace_deadline
        .is_none_or(|deadline| database_now > deadline)
    {
        return Err(SettlementRepositoryError::WindowClosed);
    }
    let updated = sqlx::query(
        "UPDATE match_participants SET state = 'aborted', reconnect_deadline = NULL \
         WHERE match_id = $1 AND account_id = $2 AND state = 'settlement_pending'",
    )
    .bind(match_id)
    .bind(account_id)
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;
    if updated.rows_affected() != 1 {
        return Err(SettlementRepositoryError::Conflict);
    }
    participant.state = ParticipantState::Aborted.as_str().to_owned();
    participant.reconnect_deadline = None;
    let record = participant.into_record().map_err(map_match_error)?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(TransitionOutcome::Applied(record))
}

pub(super) fn validate_qualification(
    match_row: &super::matchmaking::rows::MatchRow,
    qualification: &ExtractionQualification,
) -> Result<(), SettlementRepositoryError> {
    let state = match_row.parsed_state().map_err(map_match_error)?;
    if !matches!(
        state,
        MatchState::ExtractionOpen | MatchState::Settling | MatchState::Finished
    ) || match_row.config_version != qualification.config_version
        || match_row
            .extraction_open_at
            .is_none_or(|at| qualification.qualified_at < at)
        || match_row
            .hard_deadline
            .is_none_or(|at| qualification.qualified_at > at)
    {
        return Err(SettlementRepositoryError::Conflict);
    }
    Ok(())
}

pub(super) fn map_match_error(
    error: crate::ports::MatchRepositoryError,
) -> SettlementRepositoryError {
    match error {
        crate::ports::MatchRepositoryError::Unavailable => SettlementRepositoryError::Unavailable,
        crate::ports::MatchRepositoryError::Conflict
        | crate::ports::MatchRepositoryError::SeatOccupied => SettlementRepositoryError::Conflict,
    }
}

pub(super) fn unavailable(_: sqlx::Error) -> SettlementRepositoryError {
    SettlementRepositoryError::Unavailable
}

pub(super) async fn begin_write_transaction(
    pool: &PgPool,
) -> Result<Transaction<'_, Postgres>, SettlementRepositoryError> {
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    sqlx::query("SET LOCAL lock_timeout = '5s'")
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
    sqlx::query("SET LOCAL statement_timeout = '15s'")
        .execute(&mut *transaction)
        .await
        .map_err(unavailable)?;
    Ok(transaction)
}
