use std::time::Duration;

use specs::{Entities, Join, ReadStorage, WriteStorage};
use time::OffsetDateTime;
use voxelize::{Chunks, Clients, DirectionComp, MessageQueues, PositionComp, Registry};

use super::{
    authority::GameplayAuthority,
    combat_authorization::is_attack_authorized,
    combat_death::{resolve_melee_death, MeleeDeathAccess},
    combat_ordering::drain_attacks_in_stable_order,
    combat_responses::{combat_error, queue_attack_result, reject_invalid, targeting_error},
    combat_targeting::{select_combat_target, TargetCandidate},
    components::{
        CombatComp, EliminationComp, ExtractionComp, FixedEquipmentComp, HealthComp,
        MatchPlayerComp, MiningComp, ResourceInventoryComp, RoundStatsComp,
    },
    intents::AttackIntentQueue,
    messaging::{queue_death_result, queue_error, queue_health_state},
    runtime::GameplayRuntimeContext,
};
use crate::{
    contracts::{AttackResolution, ErrorCode},
    gameplay::{
        combat::{
            CombatError, DamageOutcome, SwingOutcome, BASIC_MELEE_COOLDOWN,
            BASIC_MELEE_DAMAGE_HALF_HEARTS,
        },
        drop_queue::{PendingDropQueue, SpawnedDropIds},
    },
};

pub(super) struct AttackResolutionAccess<'a, 'world> {
    pub entities: &'a Entities<'world>,
    pub context: &'a GameplayRuntimeContext,
    pub authority: &'a GameplayAuthority,
    pub clients: &'a Clients,
    pub chunks: &'a Chunks,
    pub registry: &'a Registry,
    pub now: Duration,
    pub occurred_at: OffsetDateTime,
    pub intents: &'a mut AttackIntentQueue,
    pub pending: &'a mut PendingDropQueue,
    pub spawned: &'a SpawnedDropIds,
    pub queues: &'a mut MessageQueues,
    pub players: &'a ReadStorage<'world, MatchPlayerComp>,
    pub equipment: &'a ReadStorage<'world, FixedEquipmentComp>,
    pub positions: &'a ReadStorage<'world, PositionComp>,
    pub directions: &'a ReadStorage<'world, DirectionComp>,
    pub health: &'a mut WriteStorage<'world, HealthComp>,
    pub combat: &'a mut WriteStorage<'world, CombatComp>,
    pub inventories: &'a mut WriteStorage<'world, ResourceInventoryComp>,
    pub mining: &'a mut WriteStorage<'world, MiningComp>,
    pub stats: &'a mut WriteStorage<'world, RoundStatsComp>,
    pub eliminations: &'a mut WriteStorage<'world, EliminationComp>,
    pub extractions: &'a WriteStorage<'world, ExtractionComp>,
}

pub(super) fn resolve_attacks(access: AttackResolutionAccess<'_, '_>) {
    for intent in drain_attacks_in_stable_order(access.intents, access.players) {
        let Some(attacker) = access.players.get(intent.entity) else {
            reject_invalid(
                access.context,
                access.queues,
                &intent.client_id,
                intent.request_id,
            );
            continue;
        };
        if !is_attack_authorized(
            access.entities,
            access.authority,
            access.clients,
            intent.entity,
            attacker,
            &intent.client_id,
            intent.payload.weapon_slot,
            access.equipment,
            access.health,
            access.eliminations,
            access.extractions,
        ) {
            reject_invalid(
                access.context,
                access.queues,
                &intent.client_id,
                intent.request_id,
            );
            continue;
        }

        let swing = access
            .combat
            .get_mut(intent.entity)
            .ok_or(CombatError::RevisionExhausted)
            .and_then(|state| {
                state
                    .state_mut()
                    .accept_swing(intent.sequence, access.now, BASIC_MELEE_COOLDOWN)
            });
        match swing {
            Ok(SwingOutcome::Accepted) => {}
            Ok(SwingOutcome::Cooldown { .. }) => {
                queue_error(
                    access.queues,
                    &access.context.manifest,
                    &intent.client_id,
                    intent.request_id,
                    ErrorCode::GameCooldown,
                    false,
                );
                continue;
            }
            Err(error) => {
                let (code, retryable) = combat_error(error);
                queue_error(
                    access.queues,
                    &access.context.manifest,
                    &intent.client_id,
                    intent.request_id,
                    code,
                    retryable,
                );
                continue;
            }
        }

        let Some(origin) = access
            .positions
            .get(intent.entity)
            .map(|value| value.0.to_arr())
        else {
            reject_invalid(
                access.context,
                access.queues,
                &intent.client_id,
                intent.request_id,
            );
            continue;
        };
        let Some(direction) = access
            .directions
            .get(intent.entity)
            .map(|value| value.0.to_arr())
        else {
            reject_invalid(
                access.context,
                access.queues,
                &intent.client_id,
                intent.request_id,
            );
            continue;
        };
        let candidates = (
            access.entities,
            access.players,
            access.positions,
            &*access.health,
            &*access.eliminations,
            access.extractions,
        )
            .join()
            .filter_map(
                |(entity, player, position, health, elimination, extraction)| {
                    (entity != intent.entity
                        && health.state().is_alive()
                        && elimination.record().is_none()
                        && extraction.record().is_none())
                    .then_some(TargetCandidate {
                        entity,
                        seat_id: player.seat_id(),
                        eye_position: position.0.to_arr(),
                    })
                },
            )
            .collect::<Vec<_>>();
        let target = match select_combat_target(
            access.context,
            access.chunks,
            access.registry,
            origin,
            direction,
            &candidates,
        ) {
            Ok(target) => target,
            Err(error) => {
                let (code, retryable) = targeting_error(error);
                queue_error(
                    access.queues,
                    &access.context.manifest,
                    &intent.client_id,
                    intent.request_id,
                    code,
                    retryable,
                );
                continue;
            }
        };
        let Some(target) = target else {
            queue_attack_result(
                access.context,
                access.queues,
                access.combat,
                &intent,
                AttackResolution::Miss,
            );
            continue;
        };
        let Some(victim) = access.players.get(target.entity) else {
            continue;
        };
        let Some(victim_position) = access
            .positions
            .get(target.entity)
            .map(|value| value.0.to_arr())
        else {
            continue;
        };
        let mut candidate = access.health.get(target.entity).unwrap().state().clone();
        match candidate.apply_damage(BASIC_MELEE_DAMAGE_HALF_HEARTS) {
            Ok(DamageOutcome::Damaged { .. }) => {
                *access.health.get_mut(target.entity).unwrap().state_mut() = candidate;
                queue_health_state(
                    access.queues,
                    access.context,
                    &victim.public_player_id().to_string(),
                    access.health.get(target.entity).unwrap(),
                );
                queue_attack_result(
                    access.context,
                    access.queues,
                    access.combat,
                    &intent,
                    AttackResolution::Hit,
                );
            }
            Ok(DamageOutcome::Killed) => {
                let result = resolve_melee_death(MeleeDeathAccess {
                    context: access.context,
                    now: access.now,
                    occurred_at: access.occurred_at,
                    killer_entity: intent.entity,
                    killer: attacker,
                    victim_entity: target.entity,
                    victim,
                    victim_position,
                    pending: access.pending,
                    spawned: access.spawned,
                    health: access.health,
                    inventories: access.inventories,
                    mining: access.mining,
                    stats: access.stats,
                    eliminations: access.eliminations,
                });
                let Ok(result) = result else {
                    access.authority.fail_closed();
                    queue_error(
                        access.queues,
                        &access.context.manifest,
                        &intent.client_id,
                        intent.request_id,
                        ErrorCode::ServiceUnavailable,
                        true,
                    );
                    continue;
                };
                let victim_client_id = victim.public_player_id().to_string();
                queue_health_state(
                    access.queues,
                    access.context,
                    &victim_client_id,
                    access.health.get(target.entity).unwrap(),
                );
                queue_death_result(access.queues, &victim_client_id, &result);
                queue_attack_result(
                    access.context,
                    access.queues,
                    access.combat,
                    &intent,
                    AttackResolution::Kill,
                );
            }
            Err(_) => continue,
        }
    }
}
