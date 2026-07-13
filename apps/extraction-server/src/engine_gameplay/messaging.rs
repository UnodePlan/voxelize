use serde::Serialize;
use uuid::Uuid;
use voxelize::{ClientFilter, Message, MessageQueues, MessageType, MethodProtocol};

use super::components::{FixedEquipmentComp, ResourceInventoryComp};
use crate::contracts::{ErrorCode, ExtractionManifest, ProtocolEnvelope};

const RESULT_METHOD: &str = "pvp:v1:result";
const INVENTORY_STATE_METHOD: &str = "pvp:v1:inventory-state";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlayerInventoryState {
    inventory: crate::gameplay::inventory::InventorySnapshot,
    equipment: crate::gameplay::equipment::FixedEquipmentSnapshot,
}

impl PlayerInventoryState {
    pub(super) fn new(inventory: &ResourceInventoryComp, equipment: &FixedEquipmentComp) -> Self {
        Self {
            inventory: inventory.snapshot(),
            equipment: equipment.snapshot(),
        }
    }
}

pub(super) fn queue_ok<T: Serialize>(
    queues: &mut MessageQueues,
    manifest: &ExtractionManifest,
    client_id: &str,
    request_id: Uuid,
    data: T,
) {
    if let Ok(envelope) = ProtocolEnvelope::ok(manifest, request_id, data) {
        queue_method(queues, client_id, RESULT_METHOD, &envelope);
    }
}

pub(super) fn queue_error(
    queues: &mut MessageQueues,
    manifest: &ExtractionManifest,
    client_id: &str,
    request_id: Uuid,
    code: ErrorCode,
    retryable: bool,
) {
    if let Ok(envelope) = ProtocolEnvelope::error(manifest, request_id, code, retryable) {
        queue_method(queues, client_id, RESULT_METHOD, &envelope);
    }
}

pub(super) fn queue_inventory_state(
    queues: &mut MessageQueues,
    client_id: &str,
    state: &PlayerInventoryState,
) {
    queue_method(queues, client_id, INVENTORY_STATE_METHOD, state);
}

fn queue_method<T: Serialize>(
    queues: &mut MessageQueues,
    client_id: &str,
    method_name: &str,
    payload: &T,
) {
    let Ok(payload) = serde_json::to_string(payload) else {
        return;
    };
    queues.push((
        Message::new(&MessageType::Method)
            .method(MethodProtocol {
                name: method_name.to_owned(),
                payload,
            })
            .build(),
        ClientFilter::Direct(client_id.to_owned()),
    ));
}
