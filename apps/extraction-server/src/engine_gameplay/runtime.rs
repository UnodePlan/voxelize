use std::{collections::HashMap, sync::Arc};

use specs::WorldExt;
use uuid::Uuid;
use voxelize::World;

use super::{
    authority::GameplayAuthority,
    components::{
        FixedEquipmentComp, LootDropComp, MatchPlayerComp, MiningComp, ResourceInventoryComp,
    },
    intents::{DropSlotIntentQueue, MiningIntentQueue},
    methods::install_gameplay_methods,
    mining_system::MiningResolutionSystem,
    system::GameplayRuntimeSystem,
};
use crate::{
    contracts::{bundled_manifest, ExtractionManifest, ResourceKey},
    gameplay::{
        config::GameplayConfig,
        drop_queue::{PendingDropQueue, SpawnedDropIds},
        equipment::FixedEquipment,
        harvest::HarvestedVoxelSet,
        inventory::MatchInventory,
    },
    generation::GenerationConfig,
    match_world::PlayableBounds,
    ports::MatchWorldSpec,
};

pub(super) const GAMEPLAY_SYSTEM_NAME: &str = "extraction-gameplay-runtime";
pub(super) const MINING_SYSTEM_NAME: &str = "extraction-mining-resolution";
const DEFAULT_DISPATCHER_LEAVES: &[&str] = &[
    MINING_SYSTEM_NAME,
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
    world.ecs_mut().insert(GameplayRuntimeContext::new(
        spec.match_id,
        config,
        manifest,
        spec.playable_bounds,
        generation.unbreakable_floor_y + 1,
        max_height,
        chunk_size,
    ));
    world.ecs_mut().insert(authority);
    world.ecs_mut().insert(PendingDropQueue::default());
    world.ecs_mut().insert(SpawnedDropIds::default());
    world.ecs_mut().insert(HarvestedVoxelSet::default());
    world
        .ecs_mut()
        .insert(DropSlotIntentQueue::new(config.intent_queue_capacity));
    world
        .ecs_mut()
        .insert(MiningIntentQueue::new(config.intent_queue_capacity));

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
    world.add_client_modifier(move |world, entity| {
        let client_id = world.get_id(entity);
        let Some(player) = players.get(&client_id) else {
            return;
        };
        let Ok(inventory) = MatchInventory::new(config.max_stack) else {
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
    });

    install_gameplay_methods(world);
    world
        .install_before_chunk_updating_system(MINING_SYSTEM_NAME, || MiningResolutionSystem)
        .map_err(|_| GameplayInstallError::DispatcherUnavailable)?;
    world.extend_dispatcher(|builder| {
        builder.with(
            GameplayRuntimeSystem,
            GAMEPLAY_SYSTEM_NAME,
            DEFAULT_DISPATCHER_LEAVES,
        )
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
