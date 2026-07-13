use std::time::Duration;

use specs::{Entity, WriteStorage};
use time::OffsetDateTime;

use super::{
    components::{
        EliminationComp, EliminationRecord, HealthComp, MatchPlayerComp, MiningComp,
        ResourceInventoryComp, RoundStatsComp,
    },
    runtime::GameplayRuntimeContext,
};
use crate::{
    contracts::{
        DeathCause, DeathResultData, DeathResultEnvelope, MiningIdleReason, ResourceTally,
    },
    gameplay::{
        combat::DamageOutcome,
        death::{drop_inventory_on_death_atomically, DeathDropAssets, DeathDropRequest},
        drop_queue::{PendingDropQueue, SpawnedDropIds},
        loot::ResourceBundle,
    },
};

pub(super) struct TimeoutDeathAccess<'a, 'world> {
    pub context: &'a GameplayRuntimeContext,
    pub now: Duration,
    pub occurred_at: OffsetDateTime,
    pub cause: DeathCause,
    pub victim_entity: Entity,
    pub victim: &'a MatchPlayerComp,
    pub victim_position: [f32; 3],
    pub pending: &'a mut PendingDropQueue,
    pub spawned: &'a SpawnedDropIds,
    pub health: &'a mut WriteStorage<'world, HealthComp>,
    pub inventories: &'a mut WriteStorage<'world, ResourceInventoryComp>,
    pub mining: &'a mut WriteStorage<'world, MiningComp>,
    pub stats: &'a mut WriteStorage<'world, RoundStatsComp>,
    pub eliminations: &'a mut WriteStorage<'world, EliminationComp>,
}

pub(super) fn resolve_timeout_death(
    access: TimeoutDeathAccess<'_, '_>,
) -> Result<DeathResultEnvelope, TimeoutDeathError> {
    let ready = access
        .health
        .get(access.victim_entity)
        .is_some_and(|health| health.state().is_alive())
        && access.inventories.get(access.victim_entity).is_some()
        && access.mining.get(access.victim_entity).is_some()
        && access.stats.get(access.victim_entity).is_some()
        && access
            .eliminations
            .get(access.victim_entity)
            .is_some_and(|state| state.record().is_none());
    if !ready {
        return Err(TimeoutDeathError::InvalidState);
    }

    let mut health = access
        .health
        .get(access.victim_entity)
        .unwrap()
        .state()
        .clone();
    if health.eliminate().map_err(|_| TimeoutDeathError::Prepare)? != DamageOutcome::Killed {
        return Err(TimeoutDeathError::Prepare);
    }
    let lost = inventory_contents(
        access
            .inventories
            .get(access.victim_entity)
            .unwrap()
            .snapshot(),
    )?;
    let mut stats = access
        .stats
        .get(access.victim_entity)
        .unwrap()
        .stats()
        .clone();
    stats
        .record_lost(lost)
        .map_err(|_| TimeoutDeathError::Prepare)?;
    stats
        .finish_survival(access.now)
        .map_err(|_| TimeoutDeathError::Prepare)?;
    let mut mining = access
        .mining
        .get(access.victim_entity)
        .unwrap()
        .state()
        .clone();
    mining
        .reset(MiningIdleReason::TimedOut)
        .map_err(|_| TimeoutDeathError::Prepare)?;
    let result = DeathResultEnvelope::new(
        &access.context.manifest,
        access.context.match_id,
        health.revision(),
        DeathResultData {
            cause: access.cause,
            killer_public_player_id: None,
            survived_ms: survival_ms(&stats)?,
            mined: tally(stats.mined()),
            picked_up: tally(stats.picked_up()),
            lost: tally(lost),
        },
    )
    .map_err(|_| TimeoutDeathError::Prepare)?;

    let receipt = drop_inventory_on_death_atomically(
        DeathDropRequest {
            match_id: access.context.match_id,
            seat_id: access.victim.seat_id(),
            position: [
                access.victim_position[0],
                access.victim_position[1] - 1.0,
                access.victim_position[2],
            ],
        },
        DeathDropAssets {
            inventory: access
                .inventories
                .get_mut(access.victim_entity)
                .unwrap()
                .inventory_mut(),
            pending: access.pending,
            spawned: access.spawned,
        },
    )
    .map_err(|_| TimeoutDeathError::Commit)?;
    debug_assert_eq!(receipt.contents, lost, "超时掉落必须等于终态前背包内容");

    *access
        .health
        .get_mut(access.victim_entity)
        .unwrap()
        .state_mut() = health;
    *access
        .stats
        .get_mut(access.victim_entity)
        .unwrap()
        .stats_mut() = stats;
    *access
        .mining
        .get_mut(access.victim_entity)
        .unwrap()
        .state_mut() = mining;
    let recorded = access
        .eliminations
        .get_mut(access.victim_entity)
        .unwrap()
        .eliminate(EliminationRecord {
            killer_account_id: None,
            occurred_at: access.occurred_at,
            result: result.clone(),
            notice_sent: false,
        });
    debug_assert!(recorded, "超时淘汰只能提交一次");
    Ok(result)
}

fn inventory_contents(
    snapshot: crate::gameplay::inventory::InventorySnapshot,
) -> Result<ResourceBundle, TimeoutDeathError> {
    let mut contents = ResourceBundle::default();
    for stack in snapshot.slots.into_iter().flatten() {
        contents
            .checked_merge(ResourceBundle::from_stack(stack))
            .map_err(|_| TimeoutDeathError::Prepare)?;
    }
    Ok(contents)
}

fn survival_ms(stats: &crate::gameplay::round_stats::RoundStats) -> Result<u32, TimeoutDeathError> {
    let ended = stats
        .survival_ended_at()
        .ok_or(TimeoutDeathError::Prepare)?;
    let elapsed = ended
        .checked_sub(stats.survival_started_at())
        .ok_or(TimeoutDeathError::Prepare)?;
    u32::try_from(elapsed.as_millis()).map_err(|_| TimeoutDeathError::Prepare)
}

fn tally(bundle: ResourceBundle) -> ResourceTally {
    ResourceTally {
        dirt: bundle.quantity(crate::contracts::ResourceKey::Dirt),
        gold: bundle.quantity(crate::contracts::ResourceKey::Gold),
        diamond: bundle.quantity(crate::contracts::ResourceKey::Diamond),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TimeoutDeathError {
    InvalidState,
    Prepare,
    Commit,
}
