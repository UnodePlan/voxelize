use std::time::Duration;

use specs::{Entities, Join, ReadStorage, WriteStorage};
use voxelize::{MessageQueues, PositionComp};

use super::{
    authority::GameplayAuthority,
    components::{
        EliminationComp, HealthComp, MatchPlayerComp, MiningComp, ResourceInventoryComp,
        RoundStatsComp,
    },
    forced_elimination::ForcedEliminationQueue,
    messaging::{queue_death_result, queue_health_state},
    runtime::GameplayRuntimeContext,
    timeout_death::{resolve_timeout_death, TimeoutDeathAccess, TimeoutDeathError},
};
use crate::gameplay::drop_queue::{PendingDropQueue, SpawnedDropIds};

pub(super) struct TimeoutResolutionAccess<'a, 'world> {
    pub entities: &'a Entities<'world>,
    pub context: &'a GameplayRuntimeContext,
    pub authority: &'a GameplayAuthority,
    pub now: Duration,
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
        )
            .join()
            .find_map(|(entity, player, position, health, elimination)| {
                (player.account_id() == account_id
                    && health.state().is_alive()
                    && elimination.record().is_none())
                .then_some((entity, player, position.0.to_arr()))
            });
        let Some((entity, player, position)) = target else {
            continue;
        };
        let result = resolve_timeout_death(TimeoutDeathAccess {
            context: access.context,
            now: access.now,
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
