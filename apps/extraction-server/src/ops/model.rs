use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

pub const DEFAULT_PAGE_LIMIT: u16 = 50;
pub const MAX_PAGE_LIMIT: u16 = 100;
pub const MAX_PAGE_OFFSET: u32 = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OpsPageRequest {
    pub limit: u16,
    pub offset: u32,
}

impl OpsPageRequest {
    pub fn new(limit: u16, offset: u32) -> Option<Self> {
        (limit > 0 && limit <= MAX_PAGE_LIMIT && offset <= MAX_PAGE_OFFSET)
            .then_some(Self { limit, offset })
    }
}

impl Default for OpsPageRequest {
    fn default() -> Self {
        Self {
            limit: DEFAULT_PAGE_LIMIT,
            offset: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpsPage<T> {
    pub items: Vec<T>,
    pub next_offset: Option<u32>,
}

impl<T> OpsPage<T> {
    pub(crate) fn from_fetched(mut items: Vec<T>, page: OpsPageRequest) -> Self {
        let has_more = items.len() > usize::from(page.limit);
        items.truncate(usize::from(page.limit));
        let next_offset = page.offset + u32::from(page.limit);
        Self {
            next_offset: (has_more && next_offset <= MAX_PAGE_OFFSET).then_some(next_offset),
            items,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OpsAccount {
    pub id: Uuid,
    pub status: String,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OpsMatch {
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

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct OpsResourceCounts {
    pub dirt: i64,
    pub gold: i64,
    pub diamond: i64,
}

impl OpsResourceCounts {
    pub(crate) fn insert(&mut self, key: &str, quantity: i64) -> bool {
        if quantity < 0 {
            return false;
        }
        match key {
            "dirt" => self.dirt = quantity,
            "gold" => self.gold = quantity,
            "diamond" => self.diamond = quantity,
            _ => return false,
        }
        true
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpsParticipant {
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
    pub mined: OpsResourceCounts,
    pub picked_up: OpsResourceCounts,
    pub lost: OpsResourceCounts,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OpsResourceQuantity {
    pub item_key: String,
    pub quantity: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpsSettlement {
    pub id: Uuid,
    pub match_id: Uuid,
    pub account_id: Uuid,
    pub config_version: String,
    pub total_value: i64,
    pub committed_at: OffsetDateTime,
    pub items: Vec<OpsResourceQuantity>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpsWarehouse {
    pub account_id: Uuid,
    pub balances: OpsResourceCounts,
    pub updated_at: Option<OffsetDateTime>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, sqlx::FromRow)]
#[serde(rename_all = "camelCase")]
pub struct OpsLedgerEntry {
    pub id: Uuid,
    pub account_id: Uuid,
    pub settlement_id: Uuid,
    pub item_key: String,
    pub delta: i64,
    pub created_at: OffsetDateTime,
}

#[cfg(test)]
mod tests {
    use super::{OpsPage, OpsPageRequest, MAX_PAGE_OFFSET};

    #[test]
    fn pagination_never_returns_an_unrequestable_offset() {
        let page =
            OpsPage::from_fetched(vec![1, 2], OpsPageRequest::new(1, MAX_PAGE_OFFSET).unwrap());

        assert_eq!(page.items, vec![1]);
        assert_eq!(page.next_offset, None);
    }
}
