use serde::Serialize;
use uuid::Uuid;
use voxelize::{ClientFilter, Message, MessageQueues, MessageType, MethodProtocol};

use super::{
    components::{
        CombatComp, EliminationComp, FixedEquipmentComp, HealthComp, MiningComp,
        ResourceInventoryComp,
    },
    runtime::GameplayRuntimeContext,
};
use crate::contracts::{
    AttackCursorState, DeathResultEnvelope, ErrorCode, ExtractionManifest, ExtractionStateEnvelope,
    FixedEquipmentState, HealthStateData, HealthStateEnvelope, InventoryState, InventoryStateData,
    InventoryStateEnvelope, MiningStateEnvelope, ProtocolEnvelope, ResourceStackState,
};

const RESULT_METHOD: &str = "pvp:v1:result";
const INVENTORY_STATE_METHOD: &str = "pvp:v1:inventory-state";
const MINING_STATE_METHOD: &str = "pvp:v1:mining-state";
const HEALTH_STATE_METHOD: &str = "pvp:v1:health-state";
const DEATH_RESULT_METHOD: &str = "pvp:v1:death-result";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlayerGameplayState {
    match_id: Uuid,
    #[serde(flatten)]
    assets: InventoryStateData,
    mining: MiningStateEnvelope,
    extraction: ExtractionStateEnvelope,
    health: HealthStateEnvelope,
    attack: AttackCursorState,
    death_result: Option<DeathResultEnvelope>,
}

pub(super) struct PlayerGameplayStateAccess<'a> {
    pub context: &'a GameplayRuntimeContext,
    pub inventory: &'a ResourceInventoryComp,
    pub equipment: &'a FixedEquipmentComp,
    pub mining: &'a MiningComp,
    pub extraction: ExtractionStateEnvelope,
    pub health: &'a HealthComp,
    pub combat: &'a CombatComp,
    pub elimination: &'a EliminationComp,
}

impl PlayerGameplayState {
    pub(super) fn new(access: PlayerGameplayStateAccess<'_>) -> Option<Self> {
        Some(Self {
            match_id: access.context.match_id,
            assets: player_inventory_state(access.inventory, access.equipment),
            mining: mining_state(access.context, access.mining)?,
            extraction: access.extraction,
            health: health_state(access.context, access.health)?,
            attack: AttackCursorState {
                revision: access.combat.state().revision(),
                accepted_sequence: access.combat.state().last_sequence(),
            },
            death_result: access
                .elimination
                .record()
                .map(|record| record.result.clone()),
        })
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
    context: &GameplayRuntimeContext,
    client_id: &str,
    state: InventoryStateData,
) {
    let revision = state.inventory.revision;
    if let Ok(envelope) =
        InventoryStateEnvelope::new(&context.manifest, context.match_id, revision, state)
    {
        queue_method(queues, client_id, INVENTORY_STATE_METHOD, &envelope);
    }
}

pub(super) fn player_inventory_state(
    inventory: &ResourceInventoryComp,
    equipment: &FixedEquipmentComp,
) -> InventoryStateData {
    let inventory = inventory.snapshot();
    let equipment = equipment.snapshot();
    InventoryStateData {
        inventory: InventoryState {
            slots: inventory.slots.map(|slot| {
                slot.map(|stack| ResourceStackState {
                    resource: stack.resource,
                    quantity: stack.quantity,
                })
            }),
            revision: inventory.revision,
            frozen: inventory.frozen,
            last_drop_sequence: inventory.last_drop_sequence,
        },
        equipment: FixedEquipmentState {
            pickaxe: equipment.pickaxe,
            melee_weapon: equipment.melee_weapon,
        },
    }
}

pub(super) fn queue_mining_state(
    queues: &mut MessageQueues,
    context: &GameplayRuntimeContext,
    client_id: &str,
    mining: &MiningComp,
) {
    if let Some(state) = mining_state(context, mining) {
        queue_method(queues, client_id, MINING_STATE_METHOD, &state);
    }
}

pub(super) fn queue_health_state(
    queues: &mut MessageQueues,
    context: &GameplayRuntimeContext,
    client_id: &str,
    health: &HealthComp,
) {
    if let Some(state) = health_state(context, health) {
        queue_method(queues, client_id, HEALTH_STATE_METHOD, &state);
    }
}

pub(super) fn queue_death_result(
    queues: &mut MessageQueues,
    client_id: &str,
    result: &DeathResultEnvelope,
) {
    queue_method(queues, client_id, DEATH_RESULT_METHOD, result);
}

fn mining_state(
    context: &GameplayRuntimeContext,
    mining: &MiningComp,
) -> Option<MiningStateEnvelope> {
    let required = mining
        .state()
        .active_target()
        .map(|target| context.config.mining_duration(target.resource));
    MiningStateEnvelope::new(
        &context.manifest,
        context.match_id,
        mining.state().revision(),
        mining.state().snapshot(required).ok()?,
    )
    .ok()
}

fn health_state(
    context: &GameplayRuntimeContext,
    health: &HealthComp,
) -> Option<HealthStateEnvelope> {
    let state = health.state();
    let data = if state.is_alive() {
        HealthStateData::Alive {
            current_half_hearts: state.half_hearts(),
            max_half_hearts: state.max_half_hearts(),
        }
    } else {
        HealthStateData::Dead {
            current_half_hearts: state.half_hearts(),
            max_half_hearts: state.max_half_hearts(),
        }
    };
    HealthStateEnvelope::new(&context.manifest, context.match_id, state.revision(), data).ok()
}

pub(super) fn queue_method<T: Serialize>(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        contracts::ResourceKey,
        gameplay::inventory::{DropSlotIntent, InventoryError, MatchInventory},
    };

    #[test]
    fn inventory_snapshot_exposes_the_last_consumed_drop_sequence() {
        let mut inventory = MatchInventory::new(64).unwrap();
        inventory.insert(ResourceKey::Gold, 1).unwrap();
        assert_eq!(
            inventory.propose_drop(
                23,
                DropSlotIntent {
                    slot: 0,
                    expected_revision: inventory.revision() + 1,
                },
            ),
            Err(InventoryError::RevisionMismatch)
        );
        let inventory = ResourceInventoryComp::new(inventory);
        let equipment = FixedEquipmentComp::standard();

        let state = player_inventory_state(&inventory, &equipment);

        assert_eq!(state.inventory.last_drop_sequence, Some(23));
        assert_eq!(
            serde_json::to_value(state).unwrap()["inventory"]["lastDropSequence"],
            23
        );
    }
}
