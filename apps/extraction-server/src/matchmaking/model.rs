use std::collections::HashSet;

use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::{
    terminal::{ParticipantMatchStats, ParticipantTerminalCause},
    MatchState, ParticipantState,
};
use crate::match_world::MATCH_PLAYER_CAPACITY;

pub const MATCH_SIZE: usize = MATCH_PLAYER_CAPACITY;
pub const RECONNECT_WINDOW: Duration = Duration::seconds(60);

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SeatId(u8);

impl SeatId {
    pub const fn get(self) -> u8 {
        self.0
    }
}

impl TryFrom<usize> for SeatId {
    type Error = SeatIdError;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        if value < MATCH_SIZE {
            Ok(Self(value as u8))
        } else {
            Err(SeatIdError { value })
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SeatIdError {
    pub value: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueuedPlayer {
    pub account_id: Uuid,
    pub public_player_id: Uuid,
    pub enqueued_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrozenParticipant {
    pub seat_id: SeatId,
    pub account_id: Uuid,
    pub public_player_id: Uuid,
    pub enqueued_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrozenRoster([FrozenParticipant; MATCH_SIZE]);

impl FrozenRoster {
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &FrozenParticipant> {
        self.0.iter()
    }

    pub fn participants(&self) -> &[FrozenParticipant; MATCH_SIZE] {
        &self.0
    }
}

impl TryFrom<Vec<QueuedPlayer>> for FrozenRoster {
    type Error = FrozenRosterError;

    fn try_from(players: Vec<QueuedPlayer>) -> Result<Self, Self::Error> {
        if players.len() != MATCH_SIZE {
            return Err(FrozenRosterError::WrongSize {
                actual: players.len(),
            });
        }

        let mut accounts = HashSet::with_capacity(MATCH_SIZE);
        let mut public_ids = HashSet::with_capacity(MATCH_SIZE);
        let mut participants = Vec::with_capacity(MATCH_SIZE);
        for (index, player) in players.into_iter().enumerate() {
            if !accounts.insert(player.account_id) {
                return Err(FrozenRosterError::DuplicateAccount(player.account_id));
            }
            if !public_ids.insert(player.public_player_id) {
                return Err(FrozenRosterError::DuplicatePublicPlayer(
                    player.public_player_id,
                ));
            }
            let seat_id = SeatId::try_from(index)
                .map_err(|_| FrozenRosterError::WrongSize { actual: MATCH_SIZE })?;
            participants.push(FrozenParticipant {
                seat_id,
                account_id: player.account_id,
                public_player_id: player.public_player_id,
                enqueued_at: player.enqueued_at,
            });
        }
        let participants = participants
            .try_into()
            .map_err(
                |players: Vec<FrozenParticipant>| FrozenRosterError::WrongSize {
                    actual: players.len(),
                },
            )?;
        Ok(Self(participants))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrozenRosterError {
    WrongSize { actual: usize },
    DuplicateAccount(Uuid),
    DuplicatePublicPlayer(Uuid),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchVersions {
    pub generation: String,
    pub gameplay: String,
    pub config: String,
}

impl MatchVersions {
    pub fn are_valid(&self) -> bool {
        [&self.generation, &self.gameplay, &self.config]
            .into_iter()
            .all(|version| !version.trim().is_empty())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CreatePreparingMatch {
    pub match_id: Uuid,
    pub world_name: String,
    pub seed: u64,
    pub versions: MatchVersions,
    pub created_at: OffsetDateTime,
    pub roster: FrozenRoster,
}

impl CreatePreparingMatch {
    pub fn is_valid(&self) -> bool {
        !self.world_name.trim().is_empty()
            && self.seed <= i64::MAX as u64
            && self.versions.are_valid()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivationDeadlines {
    pub extraction_open_at: OffsetDateTime,
    pub hard_deadline: OffsetDateTime,
    pub settlement_grace_deadline: OffsetDateTime,
}

impl ActivationDeadlines {
    pub fn from_started_at(started_at: OffsetDateTime) -> Option<Self> {
        let extraction_open_at = started_at.checked_add(Duration::minutes(8))?;
        let hard_deadline = started_at.checked_add(Duration::minutes(12))?;
        let settlement_grace_deadline = hard_deadline.checked_add(Duration::seconds(30))?;
        Some(Self {
            extraction_open_at,
            hard_deadline,
            settlement_grace_deadline,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchExtractionNotice {
    pub world_name: String,
    pub world_generation: String,
    pub qualification: super::ExtractionQualification,
}

impl MatchExtractionNotice {
    pub fn is_valid(&self) -> bool {
        !self.world_name.trim().is_empty()
            && !self.world_generation.trim().is_empty()
            && self.qualification.is_valid()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchRecord {
    pub match_id: Uuid,
    pub state: MatchState,
    pub world_name: String,
    pub seed: u64,
    pub versions: MatchVersions,
    pub created_at: OffsetDateTime,
    pub started_at: Option<OffsetDateTime>,
    pub extraction_open_at: Option<OffsetDateTime>,
    pub hard_deadline: Option<OffsetDateTime>,
    pub settlement_grace_deadline: Option<OffsetDateTime>,
    pub finished_at: Option<OffsetDateTime>,
    pub abort_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParticipantRecord {
    pub match_id: Uuid,
    pub account_id: Uuid,
    pub public_player_id: Uuid,
    pub seat_id: SeatId,
    pub state: ParticipantState,
    pub enqueued_at: OffsetDateTime,
    pub reconnect_deadline: Option<OffsetDateTime>,
    pub killed_by_account_id: Option<Uuid>,
    pub terminal_cause: Option<ParticipantTerminalCause>,
    pub terminal_at: Option<OffsetDateTime>,
    pub survived_ms: Option<u32>,
    pub stats: ParticipantMatchStats,
    pub extracted_at: Option<OffsetDateTime>,
    pub settlement_qualified_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StoredMatch {
    pub record: MatchRecord,
    pub participants: Vec<ParticipantRecord>,
}
