use async_trait::async_trait;
use uuid::Uuid;

use super::model::{
    OpsAccount, OpsLedgerEntry, OpsMatch, OpsPage, OpsPageRequest, OpsParticipant, OpsSettlement,
    OpsWarehouse,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpsRepositoryError {
    Invariant,
    Unavailable,
    UnsafeRole,
}

#[async_trait]
pub trait OpsRepository: Send + Sync {
    async fn account(&self, account_id: Uuid) -> Result<Option<OpsAccount>, OpsRepositoryError>;

    async fn match_record(&self, match_id: Uuid) -> Result<Option<OpsMatch>, OpsRepositoryError>;

    async fn participants(
        &self,
        match_id: Uuid,
        page: OpsPageRequest,
    ) -> Result<OpsPage<OpsParticipant>, OpsRepositoryError>;

    async fn settlement(
        &self,
        settlement_id: Uuid,
    ) -> Result<Option<OpsSettlement>, OpsRepositoryError>;

    async fn warehouse(&self, account_id: Uuid)
        -> Result<Option<OpsWarehouse>, OpsRepositoryError>;

    async fn ledger(
        &self,
        account_id: Uuid,
        page: OpsPageRequest,
    ) -> Result<OpsPage<OpsLedgerEntry>, OpsRepositoryError>;
}
