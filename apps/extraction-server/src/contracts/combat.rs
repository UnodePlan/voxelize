use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::{ContractError, ExtractionManifest, Intent, ProtocolEnvelope};

pub const MAX_HALF_HEARTS: u8 = 20;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AttackWeaponSlot {
    Melee,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttackPayload {
    pub weapon_slot: AttackWeaponSlot,
}

pub fn decode_attack_intent(
    envelope: &ProtocolEnvelope,
) -> Result<Intent<AttackPayload>, ContractError> {
    envelope.decode_intent::<AttackPayload>()
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AttackResolution {
    Miss,
    Hit,
    Kill,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttackResultData {
    pub accepted_sequence: u32,
    pub attack_revision: u32,
    pub resolution: AttackResolution,
}

pub fn decode_attack_result_data(value: Value) -> Result<AttackResultData, ContractError> {
    serde_json::from_value(value)
        .map_err(|error| ContractError::new(format!("attack result data JSON 无效: {error}")))
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "lowercase", deny_unknown_fields)]
pub enum HealthStateData {
    Alive {
        #[serde(rename = "currentHalfHearts")]
        current_half_hearts: u8,
        #[serde(rename = "maxHalfHearts")]
        max_half_hearts: u8,
    },
    Dead {
        #[serde(rename = "currentHalfHearts")]
        current_half_hearts: u8,
        #[serde(rename = "maxHalfHearts")]
        max_half_hearts: u8,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum StateEnvelopeKind {
    State,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum CombatStateStream {
    Health,
    DeathResult,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HealthStateEnvelope {
    protocol_version: u32,
    #[serde(rename = "type")]
    kind: StateEnvelopeKind,
    pub match_id: Uuid,
    stream: CombatStateStream,
    pub revision: u32,
    pub data: HealthStateData,
}

impl HealthStateEnvelope {
    pub fn new(
        manifest: &ExtractionManifest,
        match_id: Uuid,
        revision: u32,
        data: HealthStateData,
    ) -> Result<Self, ContractError> {
        let state = Self {
            protocol_version: manifest.protocol_version,
            kind: StateEnvelopeKind::State,
            match_id,
            stream: CombatStateStream::Health,
            revision,
            data,
        };
        state.validate(manifest)?;
        Ok(state)
    }

    pub fn is_dead(&self) -> bool {
        matches!(self.data, HealthStateData::Dead { .. })
    }

    pub(crate) fn validate(&self, manifest: &ExtractionManifest) -> Result<(), ContractError> {
        if self.protocol_version != manifest.protocol_version
            || self.match_id.is_nil()
            || self.stream != CombatStateStream::Health
        {
            return Err(ContractError::new(
                "health state envelope 身份、版本或流无效",
            ));
        }
        let valid = match self.data {
            HealthStateData::Alive {
                current_half_hearts,
                max_half_hearts,
            } => {
                (1..=MAX_HALF_HEARTS).contains(&current_half_hearts)
                    && max_half_hearts == MAX_HALF_HEARTS
            }
            HealthStateData::Dead {
                current_half_hearts,
                max_half_hearts,
            } => current_half_hearts == 0 && max_half_hearts == MAX_HALF_HEARTS,
        };
        if !valid {
            return Err(ContractError::new("health state 半心数值无效"));
        }
        Ok(())
    }
}

pub fn decode_health_state(
    value: Value,
    manifest: &ExtractionManifest,
) -> Result<HealthStateEnvelope, ContractError> {
    let state = serde_json::from_value::<HealthStateEnvelope>(value)
        .map_err(|error| ContractError::new(format!("health state JSON 无效: {error}")))?;
    state.validate(manifest)?;
    Ok(state)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeathCause {
    Melee,
    ReconnectTimeout,
    HardDeadline,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceTally {
    pub dirt: u32,
    pub gold: u32,
    pub diamond: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeathResultData {
    pub cause: DeathCause,
    pub killer_public_player_id: Option<Uuid>,
    pub survived_ms: u32,
    pub mined: ResourceTally,
    pub picked_up: ResourceTally,
    pub lost: ResourceTally,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeathResultEnvelope {
    protocol_version: u32,
    #[serde(rename = "type")]
    kind: StateEnvelopeKind,
    pub match_id: Uuid,
    stream: CombatStateStream,
    pub revision: u32,
    pub data: DeathResultData,
}

impl DeathResultEnvelope {
    pub fn new(
        manifest: &ExtractionManifest,
        match_id: Uuid,
        revision: u32,
        data: DeathResultData,
    ) -> Result<Self, ContractError> {
        let state = Self {
            protocol_version: manifest.protocol_version,
            kind: StateEnvelopeKind::State,
            match_id,
            stream: CombatStateStream::DeathResult,
            revision,
            data,
        };
        state.validate(manifest)?;
        Ok(state)
    }

    pub(crate) fn validate(&self, manifest: &ExtractionManifest) -> Result<(), ContractError> {
        if self.protocol_version != manifest.protocol_version
            || self.match_id.is_nil()
            || self.stream != CombatStateStream::DeathResult
        {
            return Err(ContractError::new(
                "death result envelope 身份、版本或流无效",
            ));
        }
        let killer_is_valid = match self.data.cause {
            DeathCause::Melee => self
                .data
                .killer_public_player_id
                .is_some_and(|killer| !killer.is_nil()),
            DeathCause::ReconnectTimeout | DeathCause::HardDeadline => {
                self.data.killer_public_player_id.is_none()
            }
        };
        if !killer_is_valid {
            return Err(ContractError::new("death result 击杀者与死因不一致"));
        }
        Ok(())
    }
}

pub fn decode_death_result(
    value: Value,
    manifest: &ExtractionManifest,
) -> Result<DeathResultEnvelope, ContractError> {
    let state = serde_json::from_value::<DeathResultEnvelope>(value)
        .map_err(|error| ContractError::new(format!("death result JSON 无效: {error}")))?;
    state.validate(manifest)?;
    Ok(state)
}
