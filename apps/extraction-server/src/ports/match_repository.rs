use async_trait::async_trait;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::matchmaking::{
    CreatePreparingMatch, ParticipantDeath, ParticipantRecord, ParticipantTimeout, StoredMatch,
};

use super::RepositoryProbe;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchRepositoryError {
    Conflict,
    SeatOccupied,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettlingTrigger {
    HardDeadline,
    AllParticipantsTerminal,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TransitionOutcome<T> {
    Applied(T),
    AlreadyApplied(T),
}

impl<T> TransitionOutcome<T> {
    pub fn value(&self) -> &T {
        match self {
            Self::Applied(value) | Self::AlreadyApplied(value) => value,
        }
    }

    pub fn into_value(self) -> T {
        match self {
            Self::Applied(value) | Self::AlreadyApplied(value) => value,
        }
    }

    pub const fn was_applied(&self) -> bool {
        matches!(self, Self::Applied(_))
    }
}

#[async_trait]
pub trait MatchRepository: RepositoryProbe + Send + Sync {
    async fn create_preparing(
        &self,
        command: CreatePreparingMatch,
    ) -> Result<StoredMatch, MatchRepositoryError>;

    async fn find_match(&self, match_id: Uuid)
        -> Result<Option<StoredMatch>, MatchRepositoryError>;

    async fn find_nonterminal_by_account(
        &self,
        account_id: Uuid,
    ) -> Result<Option<StoredMatch>, MatchRepositoryError>;

    /// 进程重启后无法恢复内存 World，启动时原子终止全部遗留非终态比赛。
    async fn abort_unrecoverable_matches(
        &self,
        reason: String,
        at: OffsetDateTime,
    ) -> Result<u64, MatchRepositoryError>;

    async fn activate(
        &self,
        match_id: Uuid,
        started_at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError>;

    async fn abort(
        &self,
        match_id: Uuid,
        reason: String,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError>;

    async fn mark_disconnected(
        &self,
        match_id: Uuid,
        account_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError>;

    async fn reconnect(
        &self,
        match_id: Uuid,
        account_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError>;

    async fn mark_dead(
        &self,
        death: ParticipantDeath,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError>;

    async fn mark_timed_out(
        &self,
        timeout: ParticipantTimeout,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError>;

    async fn open_extraction(
        &self,
        match_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError>;

    async fn begin_settling(
        &self,
        match_id: Uuid,
        trigger: SettlingTrigger,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError>;

    async fn finish(
        &self,
        match_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError>;
}
