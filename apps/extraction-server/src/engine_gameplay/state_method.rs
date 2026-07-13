use serde::Deserialize;
use voxelize::{MessageQueues, PositionComp, World};

use super::{
    authority::GameplayAuthority,
    components::{
        CombatComp, EliminationComp, ExtractionComp, FixedEquipmentComp, HealthComp, MiningComp,
        ResourceInventoryComp,
    },
    extraction_messaging::{extraction_state, ExtractionStateAccess},
    messaging::{queue_ok, PlayerGameplayState, PlayerGameplayStateAccess},
    methods::{decode_envelope, send_decode_error, send_error},
    runtime::GameplayRuntimeContext,
};
use crate::{contracts::ErrorCode, gameplay::extraction::ExtractionZone, generation::MapPoint};

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
    let Some(now) = authority.monotonic_now() else {
        send_error(
            world,
            &context,
            client_id,
            request_id,
            ErrorCode::ServiceUnavailable,
            true,
        );
        return;
    };
    let Some(timeline) = authority.gameplay_timeline() else {
        send_error(
            world,
            &context,
            client_id,
            request_id,
            ErrorCode::ServiceUnavailable,
            true,
        );
        return;
    };
    let zone_point = *world.read_resource::<MapPoint>();
    let inventories = world.read_component::<ResourceInventoryComp>();
    let equipment = world.read_component::<FixedEquipmentComp>();
    let mining = world.read_component::<MiningComp>();
    let health = world.read_component::<HealthComp>();
    let combat = world.read_component::<CombatComp>();
    let elimination = world.read_component::<EliminationComp>();
    let extraction = world.read_component::<ExtractionComp>();
    let positions = world.read_component::<PositionComp>();
    let state = inventories
        .get(entity)
        .zip(equipment.get(entity))
        .zip(mining.get(entity))
        .zip(health.get(entity))
        .zip(combat.get(entity))
        .zip(elimination.get(entity))
        .zip(extraction.get(entity))
        .zip(positions.get(entity))
        .and_then(
            |(
                ((((((inventory, equipment), mining), health), combat), elimination), extraction),
                position,
            )| {
                let inside = ExtractionZone::new(
                    [
                        zone_point.x as f32,
                        zone_point.y as f32,
                        zone_point.z as f32,
                    ],
                    context.config.extraction_radius,
                    context.config.extraction_half_height,
                )
                .ok()?
                .contains(position.0.to_arr());
                let extraction_state = extraction_state(ExtractionStateAccess {
                    context: &context,
                    timeline,
                    now,
                    zone_point,
                    inside,
                    alive: health.state().is_alive(),
                    eliminated: elimination.record().is_some(),
                    extraction,
                })?;
                PlayerGameplayState::new(PlayerGameplayStateAccess {
                    context: &context,
                    inventory,
                    equipment,
                    mining,
                    extraction: extraction_state,
                    health,
                    combat,
                    elimination,
                })
            },
        );
    drop(inventories);
    drop(equipment);
    drop(mining);
    drop(health);
    drop(combat);
    drop(elimination);
    drop(extraction);
    drop(positions);
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
