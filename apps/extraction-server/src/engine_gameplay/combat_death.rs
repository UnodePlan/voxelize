use std::time::Duration;

use specs::{Entity, WriteStorage};

use super::components::{
    EliminationComp, EliminationRecord, HealthComp, MatchPlayerComp, MiningComp,
    ResourceInventoryComp, RoundStatsComp,
};
use super::runtime::GameplayRuntimeContext;
use crate::{
    contracts::{
        DeathCause, DeathResultData, DeathResultEnvelope, MiningIdleReason, ResourceTally,
    },
    gameplay::{
        combat::{DamageOutcome, BASIC_MELEE_DAMAGE_HALF_HEARTS},
        death::{
            drop_inventory_on_death_atomically, DeathDropAssets, DeathDropError, DeathDropRequest,
        },
        drop_queue::{PendingDropQueue, SpawnedDropIds},
        loot::{LootError, ResourceBundle},
        mining::MiningStateError,
        round_stats::RoundStatsError,
    },
};

pub(super) struct MeleeDeathAccess<'a, 'world> {
    pub context: &'a GameplayRuntimeContext,
    pub now: Duration,
    pub killer_entity: Entity,
    pub killer: &'a MatchPlayerComp,
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

pub(super) fn resolve_melee_death(
    access: MeleeDeathAccess<'_, '_>,
) -> Result<DeathResultEnvelope, MeleeDeathError> {
    preflight_components(&access)?;

    let mut health = access
        .health
        .get(access.victim_entity)
        .unwrap()
        .state()
        .clone();
    if health.apply_damage(BASIC_MELEE_DAMAGE_HALF_HEARTS)? != DamageOutcome::Killed {
        return Err(MeleeDeathError::InvariantViolation);
    }
    let lost = inventory_contents(
        access
            .inventories
            .get(access.victim_entity)
            .unwrap()
            .snapshot(),
    )?;
    let mut victim_stats = access
        .stats
        .get(access.victim_entity)
        .unwrap()
        .stats()
        .clone();
    victim_stats.record_lost(lost)?;
    victim_stats.finish_survival(access.now)?;
    let mut killer_stats = access
        .stats
        .get(access.killer_entity)
        .unwrap()
        .stats()
        .clone();
    killer_stats.record_kill()?;
    let mut mining = access
        .mining
        .get(access.victim_entity)
        .unwrap()
        .state()
        .clone();
    mining.reset(MiningIdleReason::Disconnected)?;

    let result = DeathResultEnvelope::new(
        &access.context.manifest,
        access.context.match_id,
        health.revision(),
        DeathResultData {
            cause: DeathCause::Melee,
            killer_public_player_id: Some(access.killer.public_player_id()),
            survived_ms: survival_ms(&victim_stats)?,
            mined: tally(victim_stats.mined()),
            picked_up: tally(victim_stats.picked_up()),
            lost: tally(lost),
        },
    )
    .map_err(|_| MeleeDeathError::InvariantViolation)?;

    let receipt = drop_inventory_on_death_atomically(
        DeathDropRequest {
            match_id: access.context.match_id,
            seat_id: access.victim.seat_id(),
            position: death_drop_position(access.victim_position),
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
    )?;
    debug_assert_eq!(receipt.contents, lost, "死亡掉落必须等于终态前背包内容");

    *access
        .health
        .get_mut(access.victim_entity)
        .unwrap()
        .state_mut() = health;
    *access
        .stats
        .get_mut(access.victim_entity)
        .unwrap()
        .stats_mut() = victim_stats;
    *access
        .stats
        .get_mut(access.killer_entity)
        .unwrap()
        .stats_mut() = killer_stats;
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
            killer_account_id: Some(access.killer.account_id()),
            result: result.clone(),
            notice_sent: false,
        });
    debug_assert!(recorded, "死亡组件必须只提交一次");
    Ok(result)
}

fn preflight_components(access: &MeleeDeathAccess<'_, '_>) -> Result<(), MeleeDeathError> {
    let victim_ready = access
        .health
        .get(access.victim_entity)
        .is_some_and(|value| value.state().is_alive())
        && access.inventories.get(access.victim_entity).is_some()
        && access.mining.get(access.victim_entity).is_some()
        && access.stats.get(access.victim_entity).is_some()
        && access
            .eliminations
            .get(access.victim_entity)
            .is_some_and(|value| value.record().is_none());
    if !victim_ready || access.stats.get(access.killer_entity).is_none() {
        return Err(MeleeDeathError::InvalidState);
    }
    Ok(())
}

fn inventory_contents(
    snapshot: crate::gameplay::inventory::InventorySnapshot,
) -> Result<ResourceBundle, MeleeDeathError> {
    let mut contents = ResourceBundle::default();
    for stack in snapshot.slots.into_iter().flatten() {
        contents.checked_merge(ResourceBundle::from_stack(stack))?;
    }
    Ok(contents)
}

fn survival_ms(stats: &crate::gameplay::round_stats::RoundStats) -> Result<u32, MeleeDeathError> {
    let ended = stats
        .survival_ended_at()
        .ok_or(MeleeDeathError::InvariantViolation)?;
    let elapsed = ended
        .checked_sub(stats.survival_started_at())
        .ok_or(MeleeDeathError::InvariantViolation)?;
    u32::try_from(elapsed.as_millis()).map_err(|_| MeleeDeathError::TimeOverflow)
}

fn tally(bundle: ResourceBundle) -> ResourceTally {
    ResourceTally {
        dirt: bundle.quantity(crate::contracts::ResourceKey::Dirt),
        gold: bundle.quantity(crate::contracts::ResourceKey::Gold),
        diamond: bundle.quantity(crate::contracts::ResourceKey::Diamond),
    }
}

fn death_drop_position(eye: [f32; 3]) -> [f32; 3] {
    [eye[0], eye[1] - 1.0, eye[2]]
}

#[derive(Debug)]
pub(super) enum MeleeDeathError {
    InvalidState,
    InvariantViolation,
    TimeOverflow,
    Health,
    DeathDrop,
    Loot,
    Mining,
    Stats,
}

impl From<crate::gameplay::combat::HealthError> for MeleeDeathError {
    fn from(_: crate::gameplay::combat::HealthError) -> Self {
        Self::Health
    }
}
impl From<DeathDropError> for MeleeDeathError {
    fn from(_: DeathDropError) -> Self {
        Self::DeathDrop
    }
}
impl From<LootError> for MeleeDeathError {
    fn from(_: LootError) -> Self {
        Self::Loot
    }
}
impl From<MiningStateError> for MeleeDeathError {
    fn from(_: MiningStateError) -> Self {
        Self::Mining
    }
}
impl From<RoundStatsError> for MeleeDeathError {
    fn from(_: RoundStatsError) -> Self {
        Self::Stats
    }
}
