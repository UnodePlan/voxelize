use sqlx::{PgPool, Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    matchmaking::{MatchState, ParticipantState, StoredMatch},
    ports::{MatchRepositoryError, SettlingTrigger, TransitionOutcome},
};

use super::{
    classify_write_error,
    read::{load_match_in_transaction, lock_match},
    rows::MatchRow,
    unavailable,
};

pub(in crate::persistence::postgres) async fn open_extraction(
    pool: &PgPool,
    match_id: Uuid,
    at: OffsetDateTime,
) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    let mut row = lock_match(&mut transaction, match_id).await?;
    let state = row.parsed_state()?;
    if state == MatchState::ExtractionOpen {
        return already_applied(transaction, row).await;
    }
    state
        .transition_to(MatchState::ExtractionOpen)
        .map_err(|_| MatchRepositoryError::Conflict)?;
    if row.extraction_open_at.is_none_or(|deadline| at < deadline) {
        return Err(MatchRepositoryError::Conflict);
    }

    update_match_state(
        &mut transaction,
        match_id,
        MatchState::Active,
        MatchState::ExtractionOpen,
    )
    .await?;
    row.state = MatchState::ExtractionOpen.as_str().to_owned();
    let stored = load_match_in_transaction(&mut transaction, row).await?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(TransitionOutcome::Applied(stored))
}

pub(in crate::persistence::postgres) async fn begin_settling(
    pool: &PgPool,
    match_id: Uuid,
    trigger: SettlingTrigger,
    at: OffsetDateTime,
) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    let mut row = lock_match(&mut transaction, match_id).await?;
    let state = row.parsed_state()?;
    if state == MatchState::Settling {
        return already_applied(transaction, row).await;
    }
    state
        .transition_to(MatchState::Settling)
        .map_err(|_| MatchRepositoryError::Conflict)?;

    match trigger {
        SettlingTrigger::HardDeadline => {
            if row.hard_deadline.is_none_or(|deadline| at < deadline) {
                return Err(MatchRepositoryError::Conflict);
            }
        }
        SettlingTrigger::AllParticipantsTerminal => {}
    }
    if count_gameplay_participants(&mut transaction, match_id).await? != 0 {
        return Err(MatchRepositoryError::Conflict);
    }

    let result = sqlx::query(
        "UPDATE matches SET state = 'settling' \
         WHERE id = $1 AND state IN ('active', 'extraction_open')",
    )
    .bind(match_id)
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;
    if result.rows_affected() != 1 {
        return Err(MatchRepositoryError::Conflict);
    }
    row.state = MatchState::Settling.as_str().to_owned();
    let stored = load_match_in_transaction(&mut transaction, row).await?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(TransitionOutcome::Applied(stored))
}

pub(in crate::persistence::postgres) async fn finish(
    pool: &PgPool,
    match_id: Uuid,
    at: OffsetDateTime,
) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    let mut row = lock_match(&mut transaction, match_id).await?;
    let state = row.parsed_state()?;
    if state == MatchState::Finished {
        return already_applied(transaction, row).await;
    }
    state
        .transition_to(MatchState::Finished)
        .map_err(|_| MatchRepositoryError::Conflict)?;
    if at < row.created_at || count_gameplay_participants(&mut transaction, match_id).await? != 0 {
        return Err(MatchRepositoryError::Conflict);
    }

    let result = sqlx::query(
        "UPDATE matches SET state = 'finished', finished_at = $2 \
         WHERE id = $1 AND state = 'settling'",
    )
    .bind(match_id)
    .bind(at)
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;
    if result.rows_affected() != 1 {
        return Err(MatchRepositoryError::Conflict);
    }
    row.state = MatchState::Finished.as_str().to_owned();
    row.finished_at = Some(at);
    let stored = load_match_in_transaction(&mut transaction, row).await?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(TransitionOutcome::Applied(stored))
}

async fn already_applied(
    mut transaction: Transaction<'_, Postgres>,
    row: MatchRow,
) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
    let stored = load_match_in_transaction(&mut transaction, row).await?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(TransitionOutcome::AlreadyApplied(stored))
}

async fn update_match_state(
    transaction: &mut Transaction<'_, Postgres>,
    match_id: Uuid,
    expected: MatchState,
    next: MatchState,
) -> Result<(), MatchRepositoryError> {
    let result = sqlx::query("UPDATE matches SET state = $3 WHERE id = $1 AND state = $2")
        .bind(match_id)
        .bind(expected.as_str())
        .bind(next.as_str())
        .execute(&mut **transaction)
        .await
        .map_err(classify_write_error)?;
    (result.rows_affected() == 1)
        .then_some(())
        .ok_or(MatchRepositoryError::Conflict)
}

async fn count_gameplay_participants(
    transaction: &mut Transaction<'_, Postgres>,
    match_id: Uuid,
) -> Result<i64, MatchRepositoryError> {
    count_participants(
        transaction,
        match_id,
        &[
            ParticipantState::Waiting,
            ParticipantState::Preparing,
            ParticipantState::Active,
            ParticipantState::Disconnected,
        ],
    )
    .await
}

async fn count_participants(
    transaction: &mut Transaction<'_, Postgres>,
    match_id: Uuid,
    states: &[ParticipantState],
) -> Result<i64, MatchRepositoryError> {
    let states = states
        .iter()
        .map(|state| state.as_str())
        .collect::<Vec<_>>();
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM match_participants WHERE match_id = $1 AND state = ANY($2)",
    )
    .bind(match_id)
    .bind(states)
    .fetch_one(&mut **transaction)
    .await
    .map_err(unavailable)
}
