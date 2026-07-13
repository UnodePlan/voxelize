use specs::{Entities, Join, ReadExpect, ReadStorage, System, WriteExpect, WriteStorage};
use voxelize::{Chunks, Clients, DirectionComp, MessageQueues, PositionComp, Registry};

use super::{
    authority::GameplayAuthority,
    combat_authorization::is_attack_authorized,
    combat_death::{resolve_melee_death, MeleeDeathAccess},
    combat_ordering::drain_attacks_in_stable_order,
    combat_responses::{
        combat_error, queue_attack_result, reject_all_unavailable, reject_invalid, targeting_error,
    },
    combat_targeting::{select_combat_target, TargetCandidate},
    components::{
        CombatComp, EliminationComp, FixedEquipmentComp, HealthComp, MatchPlayerComp, MiningComp,
        ResourceInventoryComp, RoundStatsComp,
    },
    intents::AttackIntentQueue,
    messaging::{queue_death_result, queue_error, queue_health_state},
    runtime::GameplayRuntimeContext,
    timeout_resolution::{process_forced_eliminations, TimeoutResolutionAccess},
    ForcedEliminationQueue,
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

pub(super) struct CombatResolutionSystem;

impl<'a> System<'a> for CombatResolutionSystem {
    type SystemData = (
        Entities<'a>,
        ReadExpect<'a, GameplayRuntimeContext>,
        ReadExpect<'a, GameplayAuthority>,
        ReadExpect<'a, Clients>,
        ReadExpect<'a, Chunks>,
        ReadExpect<'a, Registry>,
        ReadExpect<'a, ForcedEliminationQueue>,
        WriteExpect<'a, AttackIntentQueue>,
        WriteExpect<'a, PendingDropQueue>,
        ReadExpect<'a, SpawnedDropIds>,
        WriteExpect<'a, MessageQueues>,
        ReadStorage<'a, MatchPlayerComp>,
        ReadStorage<'a, FixedEquipmentComp>,
        ReadStorage<'a, PositionComp>,
        ReadStorage<'a, DirectionComp>,
        WriteStorage<'a, HealthComp>,
        WriteStorage<'a, CombatComp>,
        WriteStorage<'a, ResourceInventoryComp>,
        WriteStorage<'a, MiningComp>,
        WriteStorage<'a, RoundStatsComp>,
        WriteStorage<'a, EliminationComp>,
    );

    fn run(&mut self, data: Self::SystemData) {
        let (
            entities,
            context,
            authority,
            clients,
            chunks,
            registry,
            forced,
            mut intents,
            mut pending,
            spawned,
            mut queues,
            players,
            equipment,
            positions,
            directions,
            mut health,
            mut combat,
            mut inventories,
            mut mining,
            mut stats,
            mut eliminations,
        ) = data;
        let Some(now) = authority.monotonic_now() else {
            reject_all_unavailable(&context, &mut intents, &mut queues);
            return;
        };

        process_forced_eliminations(TimeoutResolutionAccess {
            entities: &entities,
            context: &context,
            authority: &authority,
            now,
            forced: &forced,
            pending: &mut pending,
            spawned: &spawned,
            queues: &mut queues,
            players: &players,
            positions: &positions,
            health: &mut health,
            inventories: &mut inventories,
            mining: &mut mining,
            stats: &mut stats,
            eliminations: &mut eliminations,
        });

        for intent in drain_attacks_in_stable_order(&mut intents, &players) {
            let Some(attacker) = players.get(intent.entity) else {
                reject_invalid(&context, &mut queues, &intent.client_id, intent.request_id);
                continue;
            };
            if !is_attack_authorized(
                &entities,
                &authority,
                &clients,
                intent.entity,
                attacker,
                &intent.client_id,
                intent.payload.weapon_slot,
                &equipment,
                &health,
                &eliminations,
            ) {
                reject_invalid(&context, &mut queues, &intent.client_id, intent.request_id);
                continue;
            }

            let swing = combat
                .get_mut(intent.entity)
                .ok_or(CombatError::RevisionExhausted)
                .and_then(|state| {
                    state
                        .state_mut()
                        .accept_swing(intent.sequence, now, BASIC_MELEE_COOLDOWN)
                });
            match swing {
                Ok(SwingOutcome::Accepted) => {}
                Ok(SwingOutcome::Cooldown { .. }) => {
                    queue_error(
                        &mut queues,
                        &context.manifest,
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
                        &mut queues,
                        &context.manifest,
                        &intent.client_id,
                        intent.request_id,
                        code,
                        retryable,
                    );
                    continue;
                }
            }

            let Some(origin) = positions.get(intent.entity).map(|value| value.0.to_arr()) else {
                reject_invalid(&context, &mut queues, &intent.client_id, intent.request_id);
                continue;
            };
            let Some(direction) = directions.get(intent.entity).map(|value| value.0.to_arr())
            else {
                reject_invalid(&context, &mut queues, &intent.client_id, intent.request_id);
                continue;
            };
            let candidates = (&entities, &players, &positions, &health, &eliminations)
                .join()
                .filter_map(|(entity, player, position, health, elimination)| {
                    (entity != intent.entity
                        && health.state().is_alive()
                        && elimination.record().is_none())
                    .then_some(TargetCandidate {
                        entity,
                        seat_id: player.seat_id(),
                        eye_position: position.0.to_arr(),
                    })
                })
                .collect::<Vec<_>>();
            let target = match select_combat_target(
                &context,
                &chunks,
                &registry,
                origin,
                direction,
                &candidates,
            ) {
                Ok(target) => target,
                Err(error) => {
                    let (code, retryable) = targeting_error(error);
                    queue_error(
                        &mut queues,
                        &context.manifest,
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
                    &context,
                    &mut queues,
                    &combat,
                    &intent,
                    AttackResolution::Miss,
                );
                continue;
            };
            let Some(victim) = players.get(target.entity) else {
                continue;
            };
            let Some(victim_position) = positions.get(target.entity).map(|value| value.0.to_arr())
            else {
                continue;
            };
            let mut candidate = health.get(target.entity).unwrap().state().clone();
            match candidate.apply_damage(BASIC_MELEE_DAMAGE_HALF_HEARTS) {
                Ok(DamageOutcome::Damaged { .. }) => {
                    *health.get_mut(target.entity).unwrap().state_mut() = candidate;
                    queue_health_state(
                        &mut queues,
                        &context,
                        &victim.public_player_id().to_string(),
                        health.get(target.entity).unwrap(),
                    );
                    queue_attack_result(
                        &context,
                        &mut queues,
                        &combat,
                        &intent,
                        AttackResolution::Hit,
                    );
                }
                Ok(DamageOutcome::Killed) => {
                    let result = resolve_melee_death(MeleeDeathAccess {
                        context: &context,
                        now,
                        killer_entity: intent.entity,
                        killer: attacker,
                        victim_entity: target.entity,
                        victim,
                        victim_position,
                        pending: &mut pending,
                        spawned: &spawned,
                        health: &mut health,
                        inventories: &mut inventories,
                        mining: &mut mining,
                        stats: &mut stats,
                        eliminations: &mut eliminations,
                    });
                    let Ok(result) = result else {
                        authority.fail_closed();
                        queue_error(
                            &mut queues,
                            &context.manifest,
                            &intent.client_id,
                            intent.request_id,
                            ErrorCode::ServiceUnavailable,
                            true,
                        );
                        continue;
                    };
                    let victim_client_id = victim.public_player_id().to_string();
                    queue_health_state(
                        &mut queues,
                        &context,
                        &victim_client_id,
                        health.get(target.entity).unwrap(),
                    );
                    queue_death_result(&mut queues, &victim_client_id, &result);
                    queue_attack_result(
                        &context,
                        &mut queues,
                        &combat,
                        &intent,
                        AttackResolution::Kill,
                    );
                }
                Err(_) => continue,
            }
        }
    }
}
