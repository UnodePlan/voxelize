use std::time::Duration;

use specs::{Entities, Join, ReadStorage, WriteStorage};
use voxelize::{Chunks, Clients, DirectionComp, PositionComp, Vec3};

use super::{
    authority::GameplayAuthority,
    components::{
        EliminationComp, FixedEquipmentComp, MatchPlayerComp, MiningComp, ResourceInventoryComp,
        RoundStatsComp,
    },
    mining_dirty::MiningDirtyPlayers,
    mining_validation::{validate_mining_target, MiningValidationAccess},
    runtime::GameplayRuntimeContext,
};
use crate::{
    contracts::MiningIdleReason,
    gameplay::{
        drop_queue::{PendingDropQueue, SpawnedDropIds},
        harvest::{
            award_harvest_atomically, HarvestAssets, HarvestDestination, HarvestRequest,
            HarvestedVoxelSet,
        },
        mining::{MiningTarget, MiningTick},
    },
};

pub(super) struct MiningCompletionAccess<'a, 'world> {
    pub entities: &'a Entities<'world>,
    pub context: &'a GameplayRuntimeContext,
    pub authority: &'a GameplayAuthority,
    pub clients: &'a Clients,
    pub now: Duration,
    pub chunks: &'a mut Chunks,
    pub harvested: &'a mut HarvestedVoxelSet,
    pub pending: &'a mut PendingDropQueue,
    pub spawned: &'a SpawnedDropIds,
    pub players: &'a ReadStorage<'world, MatchPlayerComp>,
    pub equipment: &'a ReadStorage<'world, FixedEquipmentComp>,
    pub positions: &'a ReadStorage<'world, PositionComp>,
    pub directions: &'a ReadStorage<'world, DirectionComp>,
    pub eliminations: &'a ReadStorage<'world, EliminationComp>,
    pub mining: &'a mut WriteStorage<'world, MiningComp>,
    pub inventories: &'a mut WriteStorage<'world, ResourceInventoryComp>,
    pub stats: &'a mut WriteStorage<'world, RoundStatsComp>,
    pub dirty: &'a mut MiningDirtyPlayers,
}

pub(super) fn advance_mining(mut access: MiningCompletionAccess<'_, '_>) {
    let mut candidates = (
        access.entities,
        access.players,
        &*access.mining,
        access.eliminations,
    )
        .join()
        .filter_map(|(entity, player, mining, elimination)| {
            if elimination.record().is_some() {
                return None;
            }
            let target = mining.state().active_target()?;
            let required = access.context.config.mining_duration(target.resource);
            Some((
                mining.state().ready_at(required)?,
                player.seat_id(),
                entity,
                player.public_player_id().to_string(),
            ))
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|(ready_at, seat, _, _)| (*ready_at, *seat));

    for (_, _, entity, client_id) in candidates {
        let Some(player) = access.players.get(entity) else {
            continue;
        };
        if !access
            .authority
            .allows_entity(access.clients, entity, &client_id, player.account_id())
        {
            reset_player(
                &mut access,
                entity,
                &client_id,
                MiningIdleReason::Disconnected,
            );
            continue;
        }
        let Some(expected) = access
            .mining
            .get(entity)
            .and_then(|component| component.state().active_target())
        else {
            continue;
        };
        let validation = validate_current(&access, entity, expected);
        let actual = match validation {
            Ok(actual) if actual == expected => actual,
            Ok(_) => {
                reset_player(
                    &mut access,
                    entity,
                    &client_id,
                    MiningIdleReason::InvalidBlock,
                );
                continue;
            }
            Err(error) => {
                reset_player(&mut access, entity, &client_id, error.protocol().0);
                continue;
            }
        };
        let required = access.context.config.mining_duration(actual.resource);
        let tick = access.mining.get_mut(entity).map(|component| {
            component.state_mut().sample(
                access.now,
                required,
                access.context.config.mining_maintain_grace,
                access.context.config.mining_sync_interval,
            )
        });
        match tick {
            Some(Ok(MiningTick::Progressed | MiningTick::Reset)) => {
                access.dirty.mark(&client_id, entity, false);
            }
            Some(Ok(MiningTick::Ready(target))) => {
                complete_harvest(&mut access, entity, &client_id, target);
            }
            Some(Err(_)) => reset_player(
                &mut access,
                entity,
                &client_id,
                MiningIdleReason::InvalidBlock,
            ),
            _ => {}
        }
    }
}

fn validate_current(
    access: &MiningCompletionAccess<'_, '_>,
    entity: specs::Entity,
    expected: MiningTarget,
) -> Result<MiningTarget, super::mining_validation::MiningValidationError> {
    validate_mining_target(
        expected.voxel,
        MiningValidationAccess {
            context: access.context,
            chunks: access.chunks,
            harvested: access.harvested,
            equipment: access
                .equipment
                .get(entity)
                .ok_or(super::mining_validation::MiningValidationError::InvalidState)?,
            position: access
                .positions
                .get(entity)
                .ok_or(super::mining_validation::MiningValidationError::InvalidState)?,
            direction: access
                .directions
                .get(entity)
                .ok_or(super::mining_validation::MiningValidationError::InvalidState)?,
        },
    )
}

fn complete_harvest(
    access: &mut MiningCompletionAccess<'_, '_>,
    entity: specs::Entity,
    client_id: &str,
    target: MiningTarget,
) {
    let Some(component) = access.mining.get_mut(entity) else {
        return;
    };
    // 资产提交前先在副本上完成状态转换，成功后替换时不再存在失败分支。
    let Ok(completed_state) = component.state().completed() else {
        mark_reset_component(access.dirty, component, client_id, entity);
        return;
    };
    let Some(inventory) = access.inventories.get_mut(entity) else {
        mark_reset_component(access.dirty, component, client_id, entity);
        return;
    };
    let Some(mut completed_stats) = access.stats.get(entity).map(|stats| stats.stats().clone())
    else {
        mark_reset_component(access.dirty, component, client_id, entity);
        return;
    };
    if completed_stats.record_mined(target.resource, 1).is_err() {
        mark_reset_component(access.dirty, component, client_id, entity);
        return;
    }
    let award = award_harvest_atomically(
        HarvestRequest {
            match_id: access.context.match_id,
            voxel: target.voxel,
            resource: target.resource,
        },
        HarvestAssets {
            harvested: access.harvested,
            inventory: inventory.inventory_mut(),
            pending: access.pending,
            spawned: access.spawned,
        },
    );
    let Ok(destination) = award else {
        mark_reset_component(access.dirty, component, client_id, entity);
        return;
    };

    access
        .chunks
        .update_voxel(&Vec3(target.voxel.x, target.voxel.y, target.voxel.z), 0);
    *access.stats.get_mut(entity).unwrap().stats_mut() = completed_stats;
    *component.state_mut() = completed_state;
    access.dirty.mark(
        client_id,
        entity,
        matches!(destination, HarvestDestination::Inventory { .. }),
    );
}

fn mark_reset_component(
    dirty: &mut MiningDirtyPlayers,
    component: &mut MiningComp,
    client_id: &str,
    entity: specs::Entity,
) {
    if component
        .state_mut()
        .reset(MiningIdleReason::InvalidBlock)
        .unwrap_or(false)
    {
        dirty.mark(client_id, entity, false);
    }
}

fn reset_player(
    access: &mut MiningCompletionAccess<'_, '_>,
    entity: specs::Entity,
    client_id: &str,
    reason: MiningIdleReason,
) {
    if access
        .mining
        .get_mut(entity)
        .is_some_and(|component| component.state_mut().reset(reason).unwrap_or(false))
    {
        access.dirty.mark(client_id, entity, false);
    }
}
