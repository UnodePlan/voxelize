use std::{collections::HashMap, sync::Arc};

use specs::WorldExt;
use uuid::Uuid;
use voxelize::World;

use super::{
    authority::GameplayAuthority,
    components::{FixedEquipmentComp, LootDropComp, MatchPlayerComp, ResourceInventoryComp},
    intents::DropSlotIntentQueue,
    methods::install_gameplay_methods,
    system::GameplayRuntimeSystem,
};
use crate::{
    contracts::{bundled_manifest, ExtractionManifest},
    gameplay::{
        config::GameplayConfig,
        drop_queue::{PendingDropQueue, SpawnedDropIds},
        equipment::FixedEquipment,
        inventory::MatchInventory,
    },
    ports::MatchWorldSpec,
};

pub(super) const GAMEPLAY_SYSTEM_NAME: &str = "extraction-gameplay-runtime";
const DEFAULT_DISPATCHER_LEAVES: &[&str] = &[
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

    world.ecs_mut().register::<MatchPlayerComp>();
    world.ecs_mut().register::<ResourceInventoryComp>();
    world.ecs_mut().register::<FixedEquipmentComp>();
    world.ecs_mut().register::<LootDropComp>();
    world.ecs_mut().insert(GameplayRuntimeContext {
        match_id: spec.match_id,
        config,
        manifest,
    });
    world.ecs_mut().insert(authority);
    world.ecs_mut().insert(PendingDropQueue::default());
    world.ecs_mut().insert(SpawnedDropIds::default());
    world
        .ecs_mut()
        .insert(DropSlotIntentQueue::new(config.intent_queue_capacity));

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
    });

    install_gameplay_methods(world);
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
}
