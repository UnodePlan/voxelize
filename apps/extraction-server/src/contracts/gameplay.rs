use serde::Deserialize;

use super::{ContractError, Intent, ProtocolEnvelope};
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
