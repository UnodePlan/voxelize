use crate::{
    matchmaking::{
        ParticipantRecord, ParticipantState, ParticipantTerminalCause, ParticipantTimeout,
    },
    ports::{MatchRepositoryError, TransitionOutcome},
};
use sqlx::{types::Json, PgPool};

use super::{
    classify_write_error,
    participants::require_live_match,
    read::{lock_match, lock_participant},
    unavailable,
};

pub(in crate::persistence::postgres) async fn mark_timed_out(
    pool: &PgPool,
    timeout: ParticipantTimeout,
) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError> {
    if !timeout.is_valid() {
        return Err(MatchRepositoryError::Conflict);
    }
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    let match_row = lock_match(&mut transaction, timeout.match_id).await?;
    let match_state = match_row.parsed_state()?;
    let mut participant =
        lock_participant(&mut transaction, timeout.match_id, timeout.account_id).await?;
    let participant_state = participant.parsed_state()?;
    if participant_state == ParticipantState::TimedOut {
        let record = participant.into_record()?;
        if !timeout_matches_record(&timeout, &record) {
            return Err(MatchRepositoryError::Conflict);
        }
        transaction.commit().await.map_err(unavailable)?;
        return Ok(TransitionOutcome::AlreadyApplied(record));
    }
    require_live_match(match_state)?;
    participant_state
        .transition_to(ParticipantState::TimedOut)
        .map_err(|_| MatchRepositoryError::Conflict)?;
    let valid_source = match timeout.cause {
        ParticipantTerminalCause::ReconnectTimeout => {
            participant_state == ParticipantState::Disconnected
                && participant
                    .reconnect_deadline
                    .is_some_and(|deadline| timeout.occurred_at >= deadline)
        }
        ParticipantTerminalCause::HardDeadline => {
            matches!(
                participant_state,
                ParticipantState::Active | ParticipantState::Disconnected
            ) && match_row
                .hard_deadline
                .is_some_and(|deadline| timeout.occurred_at >= deadline)
        }
        ParticipantTerminalCause::Melee => false,
    };
    if !valid_source {
        return Err(MatchRepositoryError::Conflict);
    }

    let expected_states: &[&str] = match timeout.cause {
        ParticipantTerminalCause::ReconnectTimeout => &["disconnected"],
        ParticipantTerminalCause::HardDeadline => &["active", "disconnected"],
        ParticipantTerminalCause::Melee => return Err(MatchRepositoryError::Conflict),
    };
    let result = sqlx::query(
        "UPDATE match_participants SET state = 'timed_out', reconnect_deadline = NULL, \
         killed_by_account_id = NULL, terminal_cause = $3, terminal_at = $4, survived_ms = $5, \
         mined_counts = $6, pickup_counts = $7, lost_counts = $8 \
         WHERE match_id = $1 AND account_id = $2 AND state = ANY($9)",
    )
    .bind(timeout.match_id)
    .bind(timeout.account_id)
    .bind(timeout.cause.as_str())
    .bind(timeout.occurred_at)
    .bind(i64::from(timeout.survived_ms))
    .bind(Json(timeout.stats.mined))
    .bind(Json(timeout.stats.picked_up))
    .bind(Json(timeout.stats.lost))
    .bind(expected_states)
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;
    if result.rows_affected() != 1 {
        return Err(MatchRepositoryError::Conflict);
    }
    participant.state = ParticipantState::TimedOut.as_str().to_owned();
    participant.reconnect_deadline = None;
    participant.killed_by_account_id = None;
    participant.terminal_cause = Some(timeout.cause.as_str().to_owned());
    participant.terminal_at = Some(timeout.occurred_at);
    participant.survived_ms = Some(i64::from(timeout.survived_ms));
    participant.mined_counts = Json(timeout.stats.mined);
    participant.pickup_counts = Json(timeout.stats.picked_up);
    participant.lost_counts = Json(timeout.stats.lost);
    let record = participant.into_record()?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(TransitionOutcome::Applied(record))
}

fn timeout_matches_record(timeout: &ParticipantTimeout, record: &ParticipantRecord) -> bool {
    record.match_id == timeout.match_id
        && record.account_id == timeout.account_id
        && record.state == ParticipantState::TimedOut
        && record.killed_by_account_id.is_none()
        && record.terminal_cause == Some(timeout.cause)
        && record.terminal_at == Some(timeout.occurred_at)
        && record.survived_ms == Some(timeout.survived_ms)
        && record.stats == timeout.stats
        && record.reconnect_deadline.is_none()
}
