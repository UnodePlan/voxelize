use specs::{Entities, Join, ReadStorage, WriteStorage};
use voxelize::{Clients, MessageQueues, PositionComp};

use super::{
    authority::GameplayAuthority,
    components::{
        EliminationComp, ExtractionComp, FixedEquipmentComp, HealthComp, MatchPlayerComp,
        MiningComp, ResourceInventoryComp,
    },
    extraction_messaging::{extraction_state, queue_extraction_state, ExtractionStateAccess},
    messaging::{player_inventory_state, queue_inventory_state},
    runtime::GameplayRuntimeContext,
};
use crate::{
    contracts::MiningIdleReason,
    gameplay::{
        extraction::{freeze_inventory_for_extraction, ExtractionProgressOutcome, ExtractionZone},
        mining::MiningState,
    },
    generation::MapPoint,
};

pub(super) struct ExtractionResolutionAccess<'a, 'world> {
    pub entities: &'a Entities<'world>,
    pub context: &'a GameplayRuntimeContext,
    pub authority: &'a GameplayAuthority,
    pub clients: &'a Clients,
    pub zone_point: MapPoint,
    pub queues: &'a mut MessageQueues,
    pub players: &'a ReadStorage<'world, MatchPlayerComp>,
    pub equipment: &'a ReadStorage<'world, FixedEquipmentComp>,
    pub positions: &'a ReadStorage<'world, PositionComp>,
    pub health: &'a WriteStorage<'world, HealthComp>,
    pub eliminations: &'a WriteStorage<'world, EliminationComp>,
    pub extractions: &'a mut WriteStorage<'world, ExtractionComp>,
    pub inventories: &'a mut WriteStorage<'world, ResourceInventoryComp>,
    pub mining: &'a mut WriteStorage<'world, MiningComp>,
}

pub(super) fn process_extractions(access: ExtractionResolutionAccess<'_, '_>) {
    let Some(timeline) = access.authority.gameplay_timeline() else {
        access.authority.fail_closed();
        return;
    };
    let Some(now) = access.authority.monotonic_now() else {
        access.authority.fail_closed();
        return;
    };
    let zone = match ExtractionZone::new(
        [
            access.zone_point.x as f32,
            access.zone_point.y as f32,
            access.zone_point.z as f32,
        ],
        access.context.config.extraction_radius,
        access.context.config.extraction_half_height,
    ) {
        Ok(zone) => zone,
        Err(_) => {
            access.authority.fail_closed();
            return;
        }
    };

    for (entity, player, position, health, elimination, extraction) in (
        access.entities,
        access.players,
        access.positions,
        access.health,
        access.eliminations,
        &mut *access.extractions,
    )
        .join()
    {
        let client_id = player.public_player_id().to_string();
        let attached = access
            .clients
            .get(&client_id)
            .is_some_and(|client| client.attached && client.entity == entity);
        let normally_authorized =
            access
                .authority
                .allows_entity(access.clients, entity, &client_id, player.account_id());
        let deadline_closing = now >= timeline.hard_deadline;
        let eligible = timeline.extraction_open
            && attached
            && (normally_authorized || deadline_closing)
            && health.state().is_alive()
            && elimination.record().is_none();
        let inside = zone.contains(position.0.to_arr());
        if extraction.record().is_none() {
            let outcome = extraction.progress_mut().observe(
                now,
                timeline.hard_deadline,
                eligible,
                inside,
                access.context.config.extraction_hold_duration,
            );
            match outcome {
                Ok(ExtractionProgressOutcome::Qualified { qualified_at }) => {
                    let Some(before_deadline) = timeline.hard_deadline.checked_sub(qualified_at)
                    else {
                        access.authority.fail_closed();
                        return;
                    };
                    let Some(qualified_utc) = time::Duration::try_from(before_deadline)
                        .ok()
                        .and_then(|delta| timeline.hard_deadline_utc.checked_sub(delta))
                    else {
                        access.authority.fail_closed();
                        return;
                    };
                    let Some(inventory) = access.inventories.get(entity) else {
                        access.authority.fail_closed();
                        return;
                    };
                    let mut inventory_candidate = inventory.inventory().clone();
                    let qualification = match freeze_inventory_for_extraction(
                        &mut inventory_candidate,
                        access.context.match_id,
                        player.account_id(),
                        qualified_utc,
                        access.context.config.config_version,
                    ) {
                        Ok(qualification) => qualification,
                        Err(_) => {
                            access.authority.fail_closed();
                            return;
                        }
                    };
                    let Some(mining) = access.mining.get(entity) else {
                        access.authority.fail_closed();
                        return;
                    };
                    let mut mining_candidate: MiningState = mining.state().clone();
                    if mining_candidate
                        .reset(MiningIdleReason::SettlementPending)
                        .is_err()
                    {
                        access.authority.fail_closed();
                        return;
                    }
                    *access.inventories.get_mut(entity).unwrap().inventory_mut() =
                        inventory_candidate;
                    *access.mining.get_mut(entity).unwrap().state_mut() = mining_candidate;
                    if !extraction.qualify(qualification) {
                        access.authority.fail_closed();
                        return;
                    }
                    if let Some(state) = access
                        .inventories
                        .get(entity)
                        .zip(access.equipment.get(entity))
                        .map(|(inventory, equipment)| player_inventory_state(inventory, equipment))
                    {
                        queue_inventory_state(access.queues, access.context, &client_id, state);
                    }
                }
                Ok(_) => {}
                Err(_) => {
                    access.authority.fail_closed();
                    return;
                }
            }
        }

        if attached && (timeline.extraction_open || extraction.record().is_some()) {
            let Some(state) = extraction_state(ExtractionStateAccess {
                context: access.context,
                timeline,
                now,
                zone_point: access.zone_point,
                inside,
                alive: health.state().is_alive(),
                eliminated: elimination.record().is_some(),
                extraction,
            }) else {
                access.authority.fail_closed();
                return;
            };
            if extraction.should_publish(state.revision) {
                queue_extraction_state(access.queues, &client_id, &state);
            }
        }
    }
}
