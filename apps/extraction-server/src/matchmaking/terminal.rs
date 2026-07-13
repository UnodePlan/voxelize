use std::str::FromStr;

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "camelCase")]
pub struct ParticipantResourceCounts {
    pub dirt: u64,
    pub gold: u64,
    pub diamond: u64,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ParticipantMatchStats {
    pub mined: ParticipantResourceCounts,
    pub picked_up: ParticipantResourceCounts,
    pub lost: ParticipantResourceCounts,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParticipantDeath {
    pub match_id: Uuid,
    pub victim_account_id: Uuid,
    pub killer_account_id: Uuid,
    pub occurred_at: OffsetDateTime,
    pub survived_ms: u32,
    pub stats: ParticipantMatchStats,
}

impl ParticipantDeath {
    pub fn is_valid(&self) -> bool {
        !self.match_id.is_nil()
            && !self.victim_account_id.is_nil()
            && !self.killer_account_id.is_nil()
            && self.victim_account_id != self.killer_account_id
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchDeathNotice {
    pub world_name: String,
    pub world_generation: String,
    pub death: ParticipantDeath,
}

impl MatchDeathNotice {
    pub fn is_valid(&self) -> bool {
        !self.world_name.trim().is_empty()
            && !self.world_generation.trim().is_empty()
            && self.death.is_valid()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParticipantTimeout {
    pub match_id: Uuid,
    pub account_id: Uuid,
    pub cause: ParticipantTerminalCause,
    pub occurred_at: OffsetDateTime,
    pub survived_ms: u32,
    pub stats: ParticipantMatchStats,
}

impl ParticipantTimeout {
    pub fn is_valid(&self) -> bool {
        !self.match_id.is_nil() && !self.account_id.is_nil()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchTimeoutNotice {
    pub world_name: String,
    pub world_generation: String,
    pub timeout: ParticipantTimeout,
}

impl MatchTimeoutNotice {
    pub fn is_valid(&self) -> bool {
        !self.world_name.trim().is_empty()
            && !self.world_generation.trim().is_empty()
            && self.timeout.is_valid()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParticipantTerminalCause {
    Melee,
    ReconnectTimeout,
    HardDeadline,
}

impl ParticipantTerminalCause {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Melee => "melee",
            Self::ReconnectTimeout => "reconnect_timeout",
            Self::HardDeadline => "hard_deadline",
        }
    }
}

impl FromStr for ParticipantTerminalCause {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "melee" => Ok(Self::Melee),
            "reconnect_timeout" => Ok(Self::ReconnectTimeout),
            "hard_deadline" => Ok(Self::HardDeadline),
            _ => Err(()),
        }
    }
}
