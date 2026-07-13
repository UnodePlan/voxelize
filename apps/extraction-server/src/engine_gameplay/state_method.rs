use serde::Deserialize;
use voxelize::{MessageQueues, World};

use super::{
    authority::GameplayAuthority,
    components::{
        CombatComp, EliminationComp, FixedEquipmentComp, HealthComp, MiningComp,
        ResourceInventoryComp,
    },
    messaging::{queue_ok, PlayerGameplayState},
    methods::{decode_envelope, send_decode_error, send_error},
    runtime::GameplayRuntimeContext,
};
use crate::contracts::ErrorCode;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyPayload {}

pub(super) fn install_state_method(world: &mut World) {
    world.set_method_handle("pvp:v1:get-state", handle_get_state);
}

fn handle_get_state(world: &mut World, client_id: &str, payload: &str) {
    let context = {
        let context = world.read_resource::<GameplayRuntimeContext>();
        (*context).clone()
    };
    let envelope = match decode_envelope(payload, &context) {
        Ok(envelope) => envelope,
        Err(error) => {
            send_decode_error(world, &context, client_id, error);
            return;
        }
    };
    let request_id = envelope.request_id();
    if envelope.decode_intent::<EmptyPayload>().is_err() {
        send_error(
            world,
            &context,
            client_id,
            request_id,
            ErrorCode::RequestMalformed,
            false,
        );
        return;
    }
    let authority = {
        let authority = world.read_resource::<GameplayAuthority>();
        (*authority).clone()
    };
    if authority.authorize_client(world, client_id).is_none() {
        send_error(
            world,
            &context,
            client_id,
            request_id,
            ErrorCode::GameInvalidState,
            false,
        );
        return;
    }
    let Some(entity) = world.clients().get(client_id).map(|client| client.entity) else {
        return;
    };
    let inventories = world.read_component::<ResourceInventoryComp>();
    let equipment = world.read_component::<FixedEquipmentComp>();
    let mining = world.read_component::<MiningComp>();
    let health = world.read_component::<HealthComp>();
    let combat = world.read_component::<CombatComp>();
    let elimination = world.read_component::<EliminationComp>();
    let state = inventories
        .get(entity)
        .zip(equipment.get(entity))
        .zip(mining.get(entity))
        .zip(health.get(entity))
        .zip(combat.get(entity))
        .zip(elimination.get(entity))
        .and_then(
            |(((((inventory, equipment), mining), health), combat), elimination)| {
                PlayerGameplayState::new(
                    &context,
                    inventory,
                    equipment,
                    mining,
                    health,
                    combat,
                    elimination,
                )
            },
        );
    drop(inventories);
    drop(equipment);
    drop(mining);
    drop(health);
    drop(combat);
    drop(elimination);
    match state {
        Some(state) => queue_ok(
            &mut world.write_resource::<MessageQueues>(),
            &context.manifest,
            client_id,
            request_id,
            state,
        ),
        None => send_error(
            world,
            &context,
            client_id,
            request_id,
            ErrorCode::GameInvalidState,
            false,
        ),
    }
}
