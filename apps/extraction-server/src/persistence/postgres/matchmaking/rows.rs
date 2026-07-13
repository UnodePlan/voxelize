use std::str::FromStr;

use sqlx::types::Json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    matchmaking::{
        MatchRecord, MatchState, MatchVersions, ParticipantMatchStats, ParticipantRecord,
        ParticipantResourceCounts, ParticipantState, SeatId,
    },
    ports::MatchRepositoryError,
};

pub(in crate::persistence::postgres) const MATCH_COLUMNS: &str =
    "id, state, world_name, seed, generation_version, \
    gameplay_version, config_version, created_at, started_at, extraction_open_at, \
    hard_deadline, settlement_grace_deadline, finished_at, abort_reason";

pub(in crate::persistence::postgres) const PARTICIPANT_COLUMNS: &str =
    "match_id, account_id, public_player_id, \
    seat_id, state, enqueued_at, reconnect_deadline, killed_by_account_id, extracted_at, \
    settlement_qualified_at, terminal_cause, terminal_at, survived_ms, mined_counts, \
    pickup_counts, lost_counts";

#[derive(Clone, Debug, sqlx::FromRow)]
pub(in crate::persistence::postgres) struct MatchRow {
    pub id: Uuid,
    pub state: String,
    pub world_name: String,
    pub seed: i64,
    pub generation_version: String,
    pub gameplay_version: String,
    pub config_version: String,
    pub created_at: OffsetDateTime,
    pub started_at: Option<OffsetDateTime>,
    pub extraction_open_at: Option<OffsetDateTime>,
    pub hard_deadline: Option<OffsetDateTime>,
    pub settlement_grace_deadline: Option<OffsetDateTime>,
    pub finished_at: Option<OffsetDateTime>,
    pub abort_reason: Option<String>,
}

impl MatchRow {
    pub fn parsed_state(&self) -> Result<MatchState, MatchRepositoryError> {
        MatchState::from_str(&self.state).map_err(|_| MatchRepositoryError::Unavailable)
    }

    pub fn into_record(self) -> Result<MatchRecord, MatchRepositoryError> {
        let state = self.parsed_state()?;
        let seed = u64::try_from(self.seed).map_err(|_| MatchRepositoryError::Unavailable)?;
        Ok(MatchRecord {
            match_id: self.id,
            state,
            world_name: self.world_name,
            seed,
            versions: MatchVersions {
                generation: self.generation_version,
                gameplay: self.gameplay_version,
                config: self.config_version,
            },
            created_at: self.created_at,
            started_at: self.started_at,
            extraction_open_at: self.extraction_open_at,
            hard_deadline: self.hard_deadline,
            settlement_grace_deadline: self.settlement_grace_deadline,
            finished_at: self.finished_at,
            abort_reason: self.abort_reason,
        })
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub(in crate::persistence::postgres) struct ParticipantRow {
    pub match_id: Uuid,
    pub account_id: Uuid,
    pub public_player_id: Uuid,
    pub seat_id: i16,
    pub state: String,
    pub enqueued_at: OffsetDateTime,
    pub reconnect_deadline: Option<OffsetDateTime>,
    pub killed_by_account_id: Option<Uuid>,
    pub extracted_at: Option<OffsetDateTime>,
    pub settlement_qualified_at: Option<OffsetDateTime>,
    pub terminal_cause: Option<String>,
    pub terminal_at: Option<OffsetDateTime>,
    pub survived_ms: Option<i64>,
    pub mined_counts: Json<ParticipantResourceCounts>,
    pub pickup_counts: Json<ParticipantResourceCounts>,
    pub lost_counts: Json<ParticipantResourceCounts>,
}

impl ParticipantRow {
    pub fn parsed_state(&self) -> Result<ParticipantState, MatchRepositoryError> {
        ParticipantState::from_str(&self.state).map_err(|_| MatchRepositoryError::Unavailable)
    }

    pub fn into_record(self) -> Result<ParticipantRecord, MatchRepositoryError> {
        let state = self.parsed_state()?;
        let seat = usize::try_from(self.seat_id).map_err(|_| MatchRepositoryError::Unavailable)?;
        let seat_id = SeatId::try_from(seat).map_err(|_| MatchRepositoryError::Unavailable)?;
        Ok(ParticipantRecord {
            match_id: self.match_id,
            account_id: self.account_id,
            public_player_id: self.public_player_id,
            seat_id,
            state,
            enqueued_at: self.enqueued_at,
            reconnect_deadline: self.reconnect_deadline,
            killed_by_account_id: self.killed_by_account_id,
            terminal_cause: self
                .terminal_cause
                .map(|cause| cause.parse())
                .transpose()
                .map_err(|_| MatchRepositoryError::Unavailable)?,
            terminal_at: self.terminal_at,
            survived_ms: self
                .survived_ms
                .map(u32::try_from)
                .transpose()
                .map_err(|_| MatchRepositoryError::Unavailable)?,
            stats: ParticipantMatchStats {
                mined: self.mined_counts.0,
                picked_up: self.pickup_counts.0,
                lost: self.lost_counts.0,
            },
            extracted_at: self.extracted_at,
            settlement_qualified_at: self.settlement_qualified_at,
        })
    }
}
