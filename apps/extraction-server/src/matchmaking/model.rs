use std::collections::HashSet;

use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::{
    terminal::{ParticipantMatchStats, ParticipantTerminalCause},
    MatchState, ParticipantState,
};
use crate::match_world::MATCH_PLAYER_CAPACITY;

/// 生产成局人数与席位上限（DEV 可在 2..=MATCH_SIZE 间缩小）。
pub const MATCH_SIZE: usize = MATCH_PLAYER_CAPACITY;
/// DEV 默认成局人数（需开启 DEV match mode）。
pub const DEV_DEFAULT_MATCH_SIZE: usize = 2;
pub const RECONNECT_WINDOW: Duration = Duration::seconds(60);

/// 校验并返回合法 match capacity（2..=MATCH_SIZE）。
pub fn sanitize_match_capacity(size: usize) -> Option<usize> {
    (2..=MATCH_SIZE).contains(&size).then_some(size)
}

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
        // 席位索引上界仍为生产容量，允许 DEV 更短 roster
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

/// 冻结名单：长度 = 本局 capacity（2..=MATCH_SIZE）。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrozenRoster(Vec<FrozenParticipant>);

impl FrozenRoster {
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &FrozenParticipant> {
        self.0.iter()
    }

    pub fn participants(&self) -> &[FrozenParticipant] {
        &self.0
    }
}

impl TryFrom<Vec<QueuedPlayer>> for FrozenRoster {
    type Error = FrozenRosterError;

    fn try_from(players: Vec<QueuedPlayer>) -> Result<Self, Self::Error> {
        let size = players.len();
        if sanitize_match_capacity(size).is_none() {
            return Err(FrozenRosterError::WrongSize { actual: size });
        }

        let mut accounts = HashSet::with_capacity(size);
        let mut public_ids = HashSet::with_capacity(size);
        let mut participants = Vec::with_capacity(size);
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
                .map_err(|_| FrozenRosterError::WrongSize { actual: size })?;
            participants.push(FrozenParticipant {
                seat_id,
                account_id: player.account_id,
                public_player_id: player.public_player_id,
                enqueued_at: player.enqueued_at,
            });
        }
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
