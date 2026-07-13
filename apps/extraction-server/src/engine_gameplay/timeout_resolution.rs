use std::time::Duration;

use specs::{Entities, Join, ReadStorage, WriteStorage};
use time::OffsetDateTime;
use voxelize::{MessageQueues, PositionComp};

use super::{
    authority::GameplayAuthority,
    components::{
        EliminationComp, ExtractionComp, HealthComp, MatchPlayerComp, MiningComp,
        ResourceInventoryComp, RoundStatsComp,
    },
    forced_elimination::ForcedEliminationQueue,
    messaging::{queue_death_result, queue_health_state},
    runtime::GameplayRuntimeContext,
    timeout_death::{resolve_timeout_death, TimeoutDeathAccess, TimeoutDeathError},
    HardDeadlineControl, HardDeadlineRequest,
};
use crate::gameplay::drop_queue::{PendingDropQueue, SpawnedDropIds};

pub(super) struct TimeoutResolutionAccess<'a, 'world> {
    pub entities: &'a Entities<'world>,
    pub context: &'a GameplayRuntimeContext,
    pub authority: &'a GameplayAuthority,
    pub now: Duration,
    pub occurred_at: OffsetDateTime,
    pub forced: &'a ForcedEliminationQueue,
    pub pending: &'a mut PendingDropQueue,
    pub spawned: &'a SpawnedDropIds,
    pub queues: &'a mut MessageQueues,
    pub players: &'a ReadStorage<'world, MatchPlayerComp>,
    pub positions: &'a ReadStorage<'world, PositionComp>,
    pub health: &'a mut WriteStorage<'world, HealthComp>,
    pub inventories: &'a mut WriteStorage<'world, ResourceInventoryComp>,
    pub mining: &'a mut WriteStorage<'world, MiningComp>,
    pub stats: &'a mut WriteStorage<'world, RoundStatsComp>,
    pub eliminations: &'a mut WriteStorage<'world, EliminationComp>,
    pub extractions: &'a WriteStorage<'world, ExtractionComp>,
}

pub(super) fn process_forced_eliminations(access: TimeoutResolutionAccess<'_, '_>) {
    let accounts = match access.forced.drain() {
        Ok(accounts) => accounts,
        Err(_) => {
            access.authority.fail_closed();
            return;
        }
    };
    for account_id in accounts {
        let target = (
            access.entities,
            access.players,
            access.positions,
            &*access.health,
            &*access.eliminations,
            access.extractions,
        )
            .join()
            .find_map(
                |(entity, player, position, health, elimination, extraction)| {
                    (player.account_id() == account_id
                        && health.state().is_alive()
                        && elimination.record().is_none()
                        && extraction.record().is_none())
                    .then_some((entity, player, position.0.to_arr()))
                },
            );
        let Some((entity, player, position)) = target else {
            continue;
        };
        let result = resolve_timeout_death(TimeoutDeathAccess {
            context: access.context,
            now: access.now,
            occurred_at: access.occurred_at,
            cause: crate::contracts::DeathCause::ReconnectTimeout,
            victim_entity: entity,
            victim: player,
            victim_position: position,
            pending: access.pending,
            spawned: access.spawned,
            health: access.health,
            inventories: access.inventories,
            mining: access.mining,
            stats: access.stats,
            eliminations: access.eliminations,
        });
        match result {
            Ok(result) => {
                let client_id = player.public_player_id().to_string();
                queue_health_state(
                    access.queues,
                    access.context,
                    &client_id,
                    access.health.get(entity).unwrap(),
                );
                queue_death_result(access.queues, &client_id, &result);
            }
            Err(TimeoutDeathError::InvalidState) => {}
            Err(TimeoutDeathError::Prepare | TimeoutDeathError::Commit) => {
                access.authority.fail_closed();
                return;
            }
        }
    }
}

pub(super) fn process_hard_deadline_eliminations(
    access: TimeoutResolutionAccess<'_, '_>,
    request: HardDeadlineRequest,
    control: &HardDeadlineControl,
) {
    let mut targets = (
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
                (health.state().is_alive()
                    && elimination.record().is_none()
                    && extraction.record().is_none())
                .then_some((player.seat_id(), entity, player, position.0.to_arr()))
            },
        )
        .collect::<Vec<_>>();
    targets.sort_by_key(|(seat, ..)| *seat);
    for (_, entity, player, position) in targets {
        let result = resolve_timeout_death(TimeoutDeathAccess {
            context: access.context,
            now: request.monotonic_deadline,
            occurred_at: request.utc_deadline,
            cause: crate::contracts::DeathCause::HardDeadline,
            victim_entity: entity,
            victim: player,
            victim_position: position,
            pending: access.pending,
            spawned: access.spawned,
            health: access.health,
            inventories: access.inventories,
            mining: access.mining,
            stats: access.stats,
            eliminations: access.eliminations,
        });
        match result {
            Ok(result) => {
                let client_id = player.public_player_id().to_string();
                queue_health_state(
                    access.queues,
                    access.context,
                    &client_id,
                    access.health.get(entity).unwrap(),
                );
                queue_death_result(access.queues, &client_id, &result);
            }
            Err(TimeoutDeathError::InvalidState) => {}
            Err(TimeoutDeathError::Prepare | TimeoutDeathError::Commit) => {
                access.authority.fail_closed();
                return;
            }
        }
    }
    if !control.mark_terminalized(request) {
        access.authority.fail_closed();
    }
}
