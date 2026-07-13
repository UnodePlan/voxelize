use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::{ContractError, ExtractionManifest, Intent, ProtocolEnvelope, ResourceKey};
use crate::match_world::RESOURCE_BACKPACK_SLOTS;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DropSlotPayload {
    pub slot: usize,
    pub expected_inventory_revision: u32,
}

pub fn decode_drop_slot_intent(
    envelope: &ProtocolEnvelope,
) -> Result<Intent<DropSlotPayload>, ContractError> {
    let intent = envelope.decode_intent::<DropSlotPayload>()?;
    if intent.payload.slot >= RESOURCE_BACKPACK_SLOTS {
        return Err(ContractError::new("drop-slot 资源槽位越界"));
    }
    Ok(intent)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "action", rename_all = "lowercase", deny_unknown_fields)]
pub enum MiningPayload {
    Start { voxel: [i32; 3] },
    Maintain {},
    Cancel {},
}

pub fn decode_mining_intent(
    envelope: &ProtocolEnvelope,
) -> Result<Intent<MiningPayload>, ContractError> {
    envelope.decode_intent::<MiningPayload>()
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MiningIdleReason {
    Initial,
    Cancelled,
    OutOfRange,
    Occluded,
    InvalidBlock,
    Disconnected,
    TimedOut,
    Completed,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "lowercase", deny_unknown_fields)]
pub enum MiningStateData {
    Idle {
        #[serde(rename = "acceptedSequence")]
        accepted_sequence: Option<u32>,
        reason: MiningIdleReason,
    },
    Mining {
        #[serde(rename = "acceptedSequence")]
        accepted_sequence: u32,
        target: [i32; 3],
        resource: ResourceKey,
        #[serde(rename = "elapsedMs")]
        elapsed_ms: u32,
        #[serde(rename = "requiredMs")]
        required_ms: u32,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum StateEnvelopeKind {
    State,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum StateStream {
    Mining,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MiningStateEnvelope {
    protocol_version: u32,
    #[serde(rename = "type")]
    kind: StateEnvelopeKind,
    pub match_id: Uuid,
    stream: StateStream,
    pub revision: u32,
    pub data: MiningStateData,
}

impl MiningStateEnvelope {
    pub fn new(
        manifest: &ExtractionManifest,
        match_id: Uuid,
        revision: u32,
        data: MiningStateData,
    ) -> Result<Self, ContractError> {
        let state = Self {
            protocol_version: manifest.protocol_version,
            kind: StateEnvelopeKind::State,
            match_id,
            stream: StateStream::Mining,
            revision,
            data,
        };
        state.validate(manifest)?;
        Ok(state)
    }

    pub(crate) fn validate(&self, manifest: &ExtractionManifest) -> Result<(), ContractError> {
        if self.protocol_version != manifest.protocol_version || self.match_id.is_nil() {
            return Err(ContractError::new("mining state envelope 身份或版本无效"));
        }
        if let MiningStateData::Mining {
            elapsed_ms,
            required_ms,
            ..
        } = &self.data
        {
            if *required_ms == 0 || elapsed_ms > required_ms {
                return Err(ContractError::new("mining state 进度范围无效"));
            }
        }
        Ok(())
    }
}

pub fn decode_mining_state(
    value: Value,
    manifest: &ExtractionManifest,
) -> Result<MiningStateEnvelope, ContractError> {
    let state = serde_json::from_value::<MiningStateEnvelope>(value)
        .map_err(|error| ContractError::new(format!("mining state JSON 无效: {error}")))?;
    state.validate(manifest)?;
    Ok(state)
}
