use async_trait::async_trait;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::matchmaking::{
    CommitSettlement, ExtractionQualification, MatchResultRecord, ParticipantRecord,
    SettlementRecord,
};

use super::MatchRepository;
use super::{RepositoryProbe, TransitionOutcome};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SettlementRepositoryError {
    Conflict,
    WindowClosed,
    Overflow,
    Unavailable,
    OutcomeUnknown,
    Invariant,
}

#[async_trait]
pub trait SettlementRepository: RepositoryProbe + Send + Sync {
    async fn mark_settlement_pending(
        &self,
        qualification: ExtractionQualification,
    ) -> Result<TransitionOutcome<ParticipantRecord>, SettlementRepositoryError>;

    async fn commit_settlement(
        &self,
        command: CommitSettlement,
    ) -> Result<TransitionOutcome<SettlementRecord>, SettlementRepositoryError>;

    async fn find_settlement(
        &self,
        match_id: Uuid,
        account_id: Uuid,
    ) -> Result<Option<SettlementRecord>, SettlementRepositoryError>;

    async fn abort_pending_settlement(
        &self,
        match_id: Uuid,
        account_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<ParticipantRecord>, SettlementRepositoryError>;

    async fn find_match_result(
        &self,
        match_id: Uuid,
        account_id: Uuid,
    ) -> Result<Option<MatchResultRecord>, SettlementRepositoryError>;

    async fn find_latest_match_result(
        &self,
        account_id: Uuid,
    ) -> Result<Option<MatchResultRecord>, SettlementRepositoryError>;
}

pub trait MatchmakingRepository: MatchRepository + SettlementRepository {}

impl<T> MatchmakingRepository for T where T: MatchRepository + SettlementRepository + ?Sized {}
