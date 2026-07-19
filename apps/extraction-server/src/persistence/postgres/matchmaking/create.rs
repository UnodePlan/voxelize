use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    matchmaking::{
        ActivationDeadlines, CreatePreparingMatch, MatchRecord, MatchState, ParticipantRecord,
        ParticipantState, StoredMatch,
    },
    ports::{MatchRepositoryError, TransitionOutcome},
};

use super::{
    classify_write_error,
    read::{load_match_in_transaction, lock_match},
    unavailable,
};

pub(in crate::persistence::postgres) async fn create_preparing(
    pool: &PgPool,
    command: CreatePreparingMatch,
) -> Result<StoredMatch, MatchRepositoryError> {
    if !command.is_valid() {
        return Err(MatchRepositoryError::Conflict);
    }
    if let Some(stored) = super::read::find_match(pool, command.match_id).await? {
        return create_matches(&stored, &command)
            .then_some(stored)
            .ok_or(MatchRepositoryError::Conflict);
    }
    let seed = i64::try_from(command.seed).map_err(|_| MatchRepositoryError::Conflict)?;
    let mut account_ids = command
        .roster
        .iter()
        .map(|participant| participant.account_id)
        .collect::<Vec<_>>();
    // 重叠名单按同一顺序锁账号，既串行化席位互斥，也避免交叉名单形成锁环。
    account_ids.sort_unstable();

    let mut transaction = pool.begin().await.map_err(unavailable)?;
    let locked_accounts = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM accounts WHERE id = ANY($1) ORDER BY id FOR UPDATE",
    )
    .bind(&account_ids)
    .fetch_all(&mut *transaction)
    .await
    .map_err(unavailable)?;
    let roster_len = command.roster.len();
    if locked_accounts.len() != roster_len {
        return Err(MatchRepositoryError::Conflict);
    }
    let occupied = sqlx::query_scalar::<_, Uuid>(
        "SELECT account_id FROM match_participants \
         WHERE account_id = ANY($1) \
           AND state IN ('waiting', 'preparing', 'active', 'disconnected', 'settlement_pending') \
         LIMIT 1",
    )
    .bind(&account_ids)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(unavailable)?;
    if occupied.is_some() {
        return Err(MatchRepositoryError::SeatOccupied);
    }

    sqlx::query(
        "INSERT INTO matches (\
           id, state, world_name, seed, generation_version, gameplay_version, config_version, \
           created_at\
         ) VALUES ($1, 'preparing', $2, $3, $4, $5, $6, $7)",
    )
    .bind(command.match_id)
    .bind(&command.world_name)
    .bind(seed)
    .bind(&command.versions.generation)
    .bind(&command.versions.gameplay)
    .bind(&command.versions.config)
    .bind(command.created_at)
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;

    let mut participants = Vec::with_capacity(roster_len);
    for participant in command.roster.iter() {
        sqlx::query(
            "INSERT INTO match_participants (\
               match_id, account_id, public_player_id, seat_id, state, enqueued_at\
             ) VALUES ($1, $2, $3, $4, 'preparing', $5)",
        )
        .bind(command.match_id)
        .bind(participant.account_id)
        .bind(participant.public_player_id)
        .bind(i16::from(participant.seat_id.get()))
        .bind(participant.enqueued_at)
        .execute(&mut *transaction)
        .await
        .map_err(classify_write_error)?;
        participants.push(ParticipantRecord {
            match_id: command.match_id,
            account_id: participant.account_id,
            public_player_id: participant.public_player_id,
            seat_id: participant.seat_id,
            state: ParticipantState::Preparing,
            enqueued_at: participant.enqueued_at,
            reconnect_deadline: None,
            killed_by_account_id: None,
            terminal_cause: None,
            terminal_at: None,
            survived_ms: None,
            stats: Default::default(),
            extracted_at: None,
            settlement_qualified_at: None,
        });
    }
    transaction.commit().await.map_err(unavailable)?;
    Ok(StoredMatch {
        record: MatchRecord {
            match_id: command.match_id,
            state: MatchState::Preparing,
            world_name: command.world_name,
            seed: command.seed,
            versions: command.versions,
            created_at: command.created_at,
            started_at: None,
            extraction_open_at: None,
            hard_deadline: None,
            settlement_grace_deadline: None,
            finished_at: None,
            abort_reason: None,
        },
        participants,
    })
}

fn create_matches(stored: &StoredMatch, command: &CreatePreparingMatch) -> bool {
    stored.record.state == MatchState::Preparing
        && stored.record.match_id == command.match_id
        && stored.record.world_name == command.world_name
        && stored.record.seed == command.seed
        && stored.record.versions == command.versions
        && stored.record.created_at == command.created_at
        && stored.participants.len() == command.roster.len()
        && stored
            .participants
            .iter()
            .zip(command.roster.iter())
            .all(|(stored, expected)| {
                stored.state == ParticipantState::Preparing
                    && stored.seat_id == expected.seat_id
                    && stored.account_id == expected.account_id
                    && stored.public_player_id == expected.public_player_id
                    && stored.enqueued_at == expected.enqueued_at
            })
}

pub(in crate::persistence::postgres) async fn activate(
    pool: &PgPool,
    match_id: Uuid,
    started_at: OffsetDateTime,
) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
    let deadlines =
        ActivationDeadlines::from_started_at(started_at).ok_or(MatchRepositoryError::Conflict)?;
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    let mut row = lock_match(&mut transaction, match_id).await?;
    let state = row.parsed_state()?;
    if state == MatchState::Active {
        let stored = load_match_in_transaction(&mut transaction, row).await?;
        transaction.commit().await.map_err(unavailable)?;
        return Ok(TransitionOutcome::AlreadyApplied(stored));
    }
    state
        .transition_to(MatchState::Active)
        .map_err(|_| MatchRepositoryError::Conflict)?;
    if started_at < row.created_at {
        return Err(MatchRepositoryError::Conflict);
    }

    let preparing_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM match_participants \
         WHERE match_id = $1 AND state = 'preparing'",
    )
    .bind(match_id)
    .fetch_one(&mut *transaction)
    .await
    .map_err(unavailable)?;
    if preparing_count < 2 {
        return Err(MatchRepositoryError::Conflict);
    }
    let participants = sqlx::query(
        "UPDATE match_participants SET state = 'active' \
         WHERE match_id = $1 AND state = 'preparing'",
    )
    .bind(match_id)
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;
    if participants.rows_affected() != preparing_count as u64 {
        return Err(MatchRepositoryError::Conflict);
    }
    sqlx::query(
        "UPDATE matches SET state = 'active', started_at = $2, extraction_open_at = $3, \
         hard_deadline = $4, settlement_grace_deadline = $5 WHERE id = $1",
    )
    .bind(match_id)
    .bind(started_at)
    .bind(deadlines.extraction_open_at)
    .bind(deadlines.hard_deadline)
    .bind(deadlines.settlement_grace_deadline)
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;
    row.state = MatchState::Active.as_str().to_owned();
    row.started_at = Some(started_at);
    row.extraction_open_at = Some(deadlines.extraction_open_at);
    row.hard_deadline = Some(deadlines.hard_deadline);
    row.settlement_grace_deadline = Some(deadlines.settlement_grace_deadline);
    let stored = load_match_in_transaction(&mut transaction, row).await?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(TransitionOutcome::Applied(stored))
}

pub(in crate::persistence::postgres) async fn abort(
    pool: &PgPool,
    match_id: Uuid,
    reason: String,
    at: OffsetDateTime,
) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
    if reason.trim().is_empty() {
        return Err(MatchRepositoryError::Conflict);
    }
    let mut transaction = pool.begin().await.map_err(unavailable)?;
    let mut row = lock_match(&mut transaction, match_id).await?;
    let state = row.parsed_state()?;
    if state == MatchState::Aborted {
        let stored = load_match_in_transaction(&mut transaction, row).await?;
        transaction.commit().await.map_err(unavailable)?;
        return Ok(TransitionOutcome::AlreadyApplied(stored));
    }
    state
        .transition_to(MatchState::Aborted)
        .map_err(|_| MatchRepositoryError::Conflict)?;
    if at < row.created_at {
        return Err(MatchRepositoryError::Conflict);
    }

    let participant_update = sqlx::query(
        "UPDATE match_participants SET state = 'aborted', reconnect_deadline = NULL \
         WHERE match_id = $1 AND state IN (\
           'waiting', 'preparing', 'active', 'disconnected', 'settlement_pending'\
         )",
    )
    .bind(match_id)
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;
    // Preparing 时至少应有 2 名参与者被中止（与 DEV 最短 roster 一致）
    if state == MatchState::Preparing && participant_update.rows_affected() < 2 {
        return Err(MatchRepositoryError::Conflict);
    }
    sqlx::query(
        "UPDATE matches SET state = 'aborted', finished_at = $2, abort_reason = $3 WHERE id = $1",
    )
    .bind(match_id)
    .bind(at)
    .bind(&reason)
    .execute(&mut *transaction)
    .await
    .map_err(classify_write_error)?;
    row.state = MatchState::Aborted.as_str().to_owned();
    row.finished_at = Some(at);
    row.abort_reason = Some(reason);
    let stored = load_match_in_transaction(&mut transaction, row).await?;
    transaction.commit().await.map_err(unavailable)?;
    Ok(TransitionOutcome::Applied(stored))
}
