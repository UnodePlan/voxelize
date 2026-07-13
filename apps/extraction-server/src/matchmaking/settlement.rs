use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use uuid::Uuid;

use super::{MatchState, ParticipantMatchStats, ParticipantState, ParticipantTerminalCause};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct SettlementValueWeights {
    dirt: u64,
    gold: u64,
    diamond: u64,
}

impl SettlementValueWeights {
    fn resolve(config_version: &str) -> Option<Self> {
        match config_version {
            "balance-v1" => Some(SETTLEMENT_VALUE_V1),
            "balance-v2" => Some(SETTLEMENT_VALUE_V2),
            _ => None,
        }
    }
}

const SETTLEMENT_VALUE_V1: SettlementValueWeights = SettlementValueWeights {
    dirt: 1,
    gold: 10,
    diamond: 100,
};

// 新权重只由显式 balance-v2 选择，历史 V1 结算继续绑定旧权重。
const SETTLEMENT_VALUE_V2: SettlementValueWeights = SettlementValueWeights {
    dirt: 2,
    gold: 25,
    diamond: 250,
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SettlementResources {
    pub dirt: u64,
    pub gold: u64,
    pub diamond: u64,
}

impl SettlementResources {
    pub const fn new(dirt: u64, gold: u64, diamond: u64) -> Self {
        Self {
            dirt,
            gold,
            diamond,
        }
    }

    pub fn canonical_json(self) -> String {
        format!(
            "{{\"diamond\":{},\"dirt\":{},\"gold\":{}}}",
            self.diamond, self.dirt, self.gold
        )
    }

    pub fn digest(self) -> [u8; 32] {
        Sha256::digest(self.canonical_json().as_bytes()).into()
    }

    pub fn total_value(self, config_version: &str) -> Option<i64> {
        let weights = SettlementValueWeights::resolve(config_version)?;
        let value = self
            .dirt
            .checked_mul(weights.dirt)?
            .checked_add(self.gold.checked_mul(weights.gold)?)?
            .checked_add(self.diamond.checked_mul(weights.diamond)?)?;
        i64::try_from(value).ok()
    }

    pub const fn is_empty(self) -> bool {
        self.dirt == 0 && self.gold == 0 && self.diamond == 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractionQualification {
    pub match_id: Uuid,
    pub account_id: Uuid,
    pub qualified_at: OffsetDateTime,
    pub resources: SettlementResources,
    pub stats: ParticipantMatchStats,
    pub inventory_digest: [u8; 32],
    pub config_version: String,
}

impl ExtractionQualification {
    pub fn new(
        match_id: Uuid,
        account_id: Uuid,
        qualified_at: OffsetDateTime,
        resources: SettlementResources,
        stats: ParticipantMatchStats,
        config_version: String,
    ) -> Option<Self> {
        let qualification = Self {
            match_id,
            account_id,
            qualified_at,
            inventory_digest: resources.digest(),
            resources,
            stats,
            config_version,
        };
        qualification.is_valid().then_some(qualification)
    }

    pub fn is_valid(&self) -> bool {
        !self.match_id.is_nil()
            && !self.account_id.is_nil()
            && self.inventory_digest == self.resources.digest()
            && self.resources.total_value(&self.config_version).is_some()
    }

    pub fn idempotency_key(&self) -> String {
        format!("extract:v1:{}:{}", self.match_id, self.account_id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommitSettlement {
    pub settlement_id: Uuid,
    pub qualification: ExtractionQualification,
}

impl CommitSettlement {
    pub fn is_valid(&self) -> bool {
        !self.settlement_id.is_nil() && self.qualification.is_valid()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettlementRecord {
    pub settlement_id: Uuid,
    pub match_id: Uuid,
    pub account_id: Uuid,
    pub idempotency_key: String,
    pub inventory_digest: [u8; 32],
    pub config_version: String,
    pub resources: SettlementResources,
    pub total_value: i64,
    pub committed_at: OffsetDateTime,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchResultRecord {
    pub match_id: Uuid,
    pub match_state: MatchState,
    pub participant_state: ParticipantState,
    pub public_player_id: Uuid,
    pub terminal_cause: Option<ParticipantTerminalCause>,
    pub killer_public_player_id: Option<Uuid>,
    pub terminal_at: Option<OffsetDateTime>,
    pub survived_ms: Option<u32>,
    pub stats: ParticipantMatchStats,
    pub settlement: Option<SettlementRecord>,
    pub abort_reason: Option<String>,
}
