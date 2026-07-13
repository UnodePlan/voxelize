use std::{collections::HashMap, sync::Arc};

use specs::WorldExt;
use uuid::Uuid;
use voxelize::World;

use super::{
    authority::GameplayAuthority,
    combat_system::CombatResolutionSystem,
    components::{
        CombatComp, EliminationComp, ExtractionComp, FixedEquipmentComp, HealthComp, LootDropComp,
        MatchPlayerComp, MiningComp, ResourceInventoryComp, RoundStatsComp,
    },
    intents::{AttackIntentQueue, DropSlotIntentQueue, MiningIntentQueue},
    methods::install_gameplay_methods,
    mining_system::MiningResolutionSystem,
    system::GameplayRuntimeSystem,
    ForcedEliminationQueue, HardDeadlineControl,
};
use crate::{
    contracts::{bundled_manifest, ExtractionManifest, ResourceKey},
    engine_movement::{is_authoritative_movement_installed, MOVEMENT_SYSTEM_NAME},
    gameplay::{
        combat::{CombatState, HealthState},
        config::GameplayConfig,
        drop_queue::{PendingDropQueue, SpawnedDropIds},
        equipment::FixedEquipment,
        harvest::HarvestedVoxelSet,
        inventory::MatchInventory,
        round_stats::RoundStats,
    },
    generation::GenerationConfig,
    match_world::PlayableBounds,
    ports::MatchWorldSpec,
};

pub(super) const GAMEPLAY_SYSTEM_NAME: &str = "extraction-gameplay-runtime";
pub(super) const MINING_SYSTEM_NAME: &str = "extraction-mining-resolution";
pub(super) const COMBAT_SYSTEM_NAME: &str = "extraction-combat-resolution";
const DEFAULT_DISPATCHER_LEAVES: &[&str] = &[
    MINING_SYSTEM_NAME,
    COMBAT_SYSTEM_NAME,
    "chunk-saving",
    "entities-saving",
    "cleanup",
    "events",
    "walk-towards",
    "target-meta",
    "path-meta",
    "entity-tree",
];

#[derive(Clone)]
pub(super) struct GameplayRuntimeContext {
    pub match_id: Uuid,
    pub config: GameplayConfig,
    pub manifest: ExtractionManifest,
    pub playable_bounds: PlayableBounds,
    pub min_mineable_y: i32,
    pub max_height: i32,
    pub chunk_size: usize,
    resource_voxels: [(u32, ResourceKey); 3],
}

impl GameplayRuntimeContext {
    pub(super) fn new(
        match_id: Uuid,
        config: GameplayConfig,
        manifest: ExtractionManifest,
        playable_bounds: PlayableBounds,
        min_mineable_y: i32,
        max_height: i32,
        chunk_size: usize,
    ) -> Self {
        let resource_voxels = ResourceKey::ALL.map(|resource| {
            let voxel_id = manifest
                .resources
                .iter()
                .find(|definition| definition.key == resource)
                .expect("validated manifest contains every resource")
                .voxel_id;
            (voxel_id, resource)
        });
        Self {
            match_id,
            config,
            manifest,
            playable_bounds,
            min_mineable_y,
            max_height,
            chunk_size,
            resource_voxels,
        }
    }

    pub(super) fn resource_for_voxel(&self, voxel_id: u32) -> Option<ResourceKey> {
        self.resource_voxels
            .iter()
            .find_map(|(id, resource)| (*id == voxel_id).then_some(*resource))
    }
}

pub(crate) fn install_gameplay_runtime(
    world: &mut World,
    spec: &MatchWorldSpec,
    authority: GameplayAuthority,
) -> Result<(), GameplayInstallError> {
    let config = GameplayConfig::resolve(&spec.gameplay_version, &spec.config_version)
        .copied()
        .ok_or(GameplayInstallError::UnsupportedVersion)?;
    let equipment = FixedEquipment::standard().snapshot();
    if config.inventory_slots != spec.loadout.resource_backpack_slots
        || equipment.pickaxe != spec.loadout.pickaxe
        || equipment.melee_weapon != spec.loadout.melee_weapon
        || spec.loadout.max_health_half_hearts != crate::contracts::MAX_HALF_HEARTS
    {
        return Err(GameplayInstallError::LoadoutMismatch);
    }
    let manifest = bundled_manifest().map_err(|_| GameplayInstallError::InvalidManifest)?;
    if manifest
        .resources
        .iter()
        .any(|resource| resource.max_stack != config.max_stack)
    {
        return Err(GameplayInstallError::LoadoutMismatch);
    }
    let generation = GenerationConfig::resolve(&spec.generation_version, &spec.config_version)
        .ok_or(GameplayInstallError::UnsupportedVersion)?;
    if !config.mining_reach.is_finite()
        || config.mining_reach <= 0.0
        || !config.mining_eye_offset.is_finite()
        || config.mining_maintain_grace.is_zero()
        || config.mining_sync_interval.as_millis() == 0
        || ResourceKey::ALL
            .into_iter()
            .any(|resource| config.mining_duration(resource).as_millis() == 0)
    {
        return Err(GameplayInstallError::InvalidConfig);
    }
    let max_height =
        i32::try_from(generation.max_height).map_err(|_| GameplayInstallError::InvalidConfig)?;
    let chunk_size = world.config().chunk_size;

    world.ecs_mut().register::<MatchPlayerComp>();
    world.ecs_mut().register::<ResourceInventoryComp>();
    world.ecs_mut().register::<FixedEquipmentComp>();
    world.ecs_mut().register::<MiningComp>();
    world.ecs_mut().register::<LootDropComp>();
    world.ecs_mut().register::<HealthComp>();
    world.ecs_mut().register::<CombatComp>();
    world.ecs_mut().register::<RoundStatsComp>();
    world.ecs_mut().register::<EliminationComp>();
    world.ecs_mut().register::<ExtractionComp>();
    world.ecs_mut().insert(GameplayRuntimeContext::new(
        spec.match_id,
        config,
        manifest,
        spec.playable_bounds,
        generation.unbreakable_floor_y + 1,
        max_height,
        chunk_size,
    ));
    let player_clock = authority.clone();
    world.ecs_mut().insert(authority);
    world.ecs_mut().insert(PendingDropQueue::default());
    world.ecs_mut().insert(SpawnedDropIds::default());
    world.ecs_mut().insert(HarvestedVoxelSet::default());
    world.ecs_mut().insert(ForcedEliminationQueue::default());
    world.ecs_mut().insert(HardDeadlineControl::default());
    world
        .ecs_mut()
        .insert(DropSlotIntentQueue::new(config.intent_queue_capacity));
    world
        .ecs_mut()
        .insert(MiningIntentQueue::new(config.intent_queue_capacity));
    world
        .ecs_mut()
        .insert(AttackIntentQueue::new(config.intent_queue_capacity));

    let players = spec
        .roster
        .iter()
        .map(|participant| {
            (
                participant.public_player_id.to_string(),
                MatchPlayerComp::new(
                    participant.account_id,
                    participant.public_player_id,
                    participant.seat_id,
                ),
            )
        })
        .collect::<HashMap<_, _>>();
    let players = Arc::new(players);
    let max_health_half_hearts = spec.loadout.max_health_half_hearts;
    world.add_client_modifier(move |world, entity| {
        let client_id = world.get_id(entity);
        let Some(player) = players.get(&client_id) else {
            return;
        };
        let Ok(inventory) = MatchInventory::new(config.max_stack) else {
            return;
        };
        let Ok(health) = HealthState::new(max_health_half_hearts) else {
            return;
        };
        let Some(now) = player_clock.monotonic_now() else {
            return;
        };
        world.add(
            entity,
            MatchPlayerComp::new(
                player.account_id(),
                player.public_player_id(),
                player.seat_id(),
            ),
        );
        world.add(entity, ResourceInventoryComp::new(inventory));
        world.add(entity, FixedEquipmentComp::standard());
        world.add(entity, MiningComp::new());
        world.add(entity, HealthComp::new(health));
        world.add(entity, CombatComp::new(CombatState::default()));
        world.add(entity, RoundStatsComp::new(RoundStats::new(now)));
        world.add(entity, EliminationComp::alive());
        world.add(entity, ExtractionComp::default());
    });

    install_gameplay_methods(world);
    world
        .install_before_chunk_updating_system(MINING_SYSTEM_NAME, || MiningResolutionSystem)
        .map_err(|_| GameplayInstallError::DispatcherUnavailable)?;
    world
        .install_before_broadcast_system(COMBAT_SYSTEM_NAME, || CombatResolutionSystem)
        .map_err(|_| GameplayInstallError::DispatcherUnavailable)?;
    let mut dependencies = DEFAULT_DISPATCHER_LEAVES.to_vec();
    if is_authoritative_movement_installed(world) {
        dependencies.push(MOVEMENT_SYSTEM_NAME);
    }
    world.extend_dispatcher(move |builder| {
        builder.with(GameplayRuntimeSystem, GAMEPLAY_SYSTEM_NAME, &dependencies)
    });
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GameplayInstallError {
    UnsupportedVersion,
    InvalidManifest,
    LoadoutMismatch,
    InvalidConfig,
    DispatcherUnavailable,
}
