use time::OffsetDateTime;
use uuid::Uuid;

use crate::ops::{
    model::{OpsParticipant, OpsResourceCounts, OpsResourceQuantity, OpsSettlement},
    repository::OpsRepositoryError,
};

#[derive(sqlx::FromRow)]
pub(super) struct ParticipantRow {
    match_id: Uuid,
    account_id: Uuid,
    public_player_id: Uuid,
    seat_id: i16,
    state: String,
    enqueued_at: OffsetDateTime,
    reconnect_deadline: Option<OffsetDateTime>,
    killed_by_account_id: Option<Uuid>,
    extracted_at: Option<OffsetDateTime>,
    settlement_qualified_at: Option<OffsetDateTime>,
    terminal_cause: Option<String>,
    terminal_at: Option<OffsetDateTime>,
    survived_ms: Option<i64>,
    mined_counts: serde_json::Value,
    pickup_counts: serde_json::Value,
    lost_counts: serde_json::Value,
}

impl ParticipantRow {
    pub(super) fn into_model(self) -> Result<OpsParticipant, OpsRepositoryError> {
        if !(0..=9).contains(&self.seat_id) || self.survived_ms.is_some_and(|value| value < 0) {
            return Err(OpsRepositoryError::Invariant);
        }
        Ok(OpsParticipant {
            match_id: self.match_id,
            account_id: self.account_id,
            public_player_id: self.public_player_id,
            seat_id: self.seat_id,
            state: self.state,
            enqueued_at: self.enqueued_at,
            reconnect_deadline: self.reconnect_deadline,
            killed_by_account_id: self.killed_by_account_id,
            extracted_at: self.extracted_at,
            settlement_qualified_at: self.settlement_qualified_at,
            terminal_cause: self.terminal_cause,
            terminal_at: self.terminal_at,
            survived_ms: self.survived_ms,
            mined: parse_counts(self.mined_counts)?,
            picked_up: parse_counts(self.pickup_counts)?,
            lost: parse_counts(self.lost_counts)?,
        })
    }
}

#[derive(sqlx::FromRow)]
pub(super) struct SettlementRow {
    id: Uuid,
    match_id: Uuid,
    account_id: Uuid,
    config_version: String,
    total_value: i64,
    committed_at: OffsetDateTime,
}

impl SettlementRow {
    pub(super) fn into_model(
        self,
        items: Vec<OpsResourceQuantity>,
    ) -> Result<OpsSettlement, OpsRepositoryError> {
        let mut seen = [false; 3];
        let invalid_item = items.iter().any(|item| {
            let index = match item.item_key.as_str() {
                "dirt" => 0,
                "gold" => 1,
                "diamond" => 2,
                _ => return true,
            };
            let duplicate = seen[index];
            seen[index] = true;
            duplicate || item.quantity <= 0
        });
        if self.total_value < 0 || invalid_item {
            return Err(OpsRepositoryError::Invariant);
        }
        Ok(OpsSettlement {
            id: self.id,
            match_id: self.match_id,
            account_id: self.account_id,
            config_version: self.config_version,
            total_value: self.total_value,
            committed_at: self.committed_at,
            items,
        })
    }
}

#[derive(sqlx::FromRow)]
pub(super) struct WarehouseRow {
    pub(super) item_key: String,
    pub(super) quantity: i64,
    pub(super) updated_at: OffsetDateTime,
}

fn parse_counts(value: serde_json::Value) -> Result<OpsResourceCounts, OpsRepositoryError> {
    let counts: OpsResourceCounts =
        serde_json::from_value(value).map_err(|_| OpsRepositoryError::Invariant)?;
    (counts.dirt >= 0 && counts.gold >= 0 && counts.diamond >= 0)
        .then_some(counts)
        .ok_or(OpsRepositoryError::Invariant)
}
