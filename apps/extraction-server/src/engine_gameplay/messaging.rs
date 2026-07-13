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
    AttackCursorState, DeathResultEnvelope, ErrorCode, ExtractionManifest, HealthStateData,
    HealthStateEnvelope, MiningStateEnvelope, ProtocolEnvelope,
};

const RESULT_METHOD: &str = "pvp:v1:result";
const INVENTORY_STATE_METHOD: &str = "pvp:v1:inventory-state";
const MINING_STATE_METHOD: &str = "pvp:v1:mining-state";
const HEALTH_STATE_METHOD: &str = "pvp:v1:health-state";
const DEATH_RESULT_METHOD: &str = "pvp:v1:death-result";

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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct PlayerGameplayState {
    match_id: Uuid,
    #[serde(flatten)]
    assets: PlayerInventoryState,
    mining: MiningStateEnvelope,
    health: HealthStateEnvelope,
    attack: AttackCursorState,
    death_result: Option<DeathResultEnvelope>,
}

impl PlayerGameplayState {
    pub(super) fn new(
        context: &GameplayRuntimeContext,
        inventory: &ResourceInventoryComp,
        equipment: &FixedEquipmentComp,
        mining: &MiningComp,
        health: &HealthComp,
        combat: &CombatComp,
        elimination: &EliminationComp,
    ) -> Option<Self> {
        Some(Self {
            match_id: context.match_id,
            assets: PlayerInventoryState::new(inventory, equipment),
            mining: mining_state(context, mining)?,
            health: health_state(context, health)?,
            attack: AttackCursorState {
                revision: combat.state().revision(),
                accepted_sequence: combat.state().last_sequence(),
            },
            death_result: elimination.record().map(|record| record.result.clone()),
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
    client_id: &str,
    state: &PlayerInventoryState,
) {
    queue_method(queues, client_id, INVENTORY_STATE_METHOD, state);
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
