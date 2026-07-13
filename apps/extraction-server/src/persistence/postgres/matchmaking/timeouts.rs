use sqlx::{types::Json, PgPool};
use time::OffsetDateTime;

use crate::{
    matchmaking::{ParticipantRecord, ParticipantState, ParticipantTimeout},
    ports::{MatchRepositoryError, TransitionOutcome},
};

use super::{
    classify_write_error,
    participants::require_live_match,
    read::{lock_match, lock_participant},
    unavailable,
};

pub(in crate::persistence::postgres) async fn mark_timed_out(
    pool: &PgPool,
    timeout: ParticipantTimeout,
    at: OffsetDateTime,
) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError> {
    if !timeout.is_valid() {
        return Err(MatchRepositoryError::Conflict);
    }
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    let match_state = lock_match(&mut transaction, timeout.match_id)
        .await?
        .parsed_state()?;
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
    if participant
        .reconnect_deadline
        .is_none_or(|deadline| at < deadline)
    {
        return Err(MatchRepositoryError::Conflict);
    }

    // deadline、断线状态与统计在同一条 CAS 中提交，避免超时结果只落一半。
    let result = sqlx::query(
        "UPDATE match_participants SET state = 'timed_out', reconnect_deadline = NULL, \
         killed_by_account_id = NULL, mined_counts = $4, pickup_counts = $5, lost_counts = $6 \
         WHERE match_id = $1 AND account_id = $2 AND state = 'disconnected' \
           AND reconnect_deadline <= $3",
    )
    .bind(timeout.match_id)
    .bind(timeout.account_id)
    .bind(at)
    .bind(Json(timeout.stats.mined))
    .bind(Json(timeout.stats.picked_up))
    .bind(Json(timeout.stats.lost))
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;
    if result.rows_affected() != 1 {
        return Err(MatchRepositoryError::Conflict);
    }
    participant.state = ParticipantState::TimedOut.as_str().to_owned();
    participant.reconnect_deadline = None;
    participant.killed_by_account_id = None;
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
        && record.stats == timeout.stats
        && record.reconnect_deadline.is_none()
}
