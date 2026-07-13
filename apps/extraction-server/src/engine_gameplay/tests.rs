use std::time::Duration;

use specs::{Builder, Join, RunNow, WorldExt};
use time::OffsetDateTime;
use uuid::Uuid;
use voxelize::{DirectionComp, PositionComp, World, WorldConfig};

use super::{
    authority::GameplayAuthority,
    components::{
        CombatComp, EliminationComp, ExtractionComp, FixedEquipmentComp, HealthComp, LootDropComp,
        MatchPlayerComp, MiningComp, ResourceInventoryComp, RoundStatsComp,
    },
    intents::{DropSlotIntentQueue, QueuedDropSlotIntent},
    runtime::{install_gameplay_runtime, GameplayInstallError},
    system::GameplayRuntimeSystem,
};
use crate::{
    contracts::{DropSlotPayload, ResourceKey},
    gameplay::{
        combat::{CombatState, HealthState},
        drop_queue::PendingDropQueue,
        inventory::{MatchInventory, ResourceStack},
        loot::{DropId, LootDrop, ResourceBundle},
        round_stats::RoundStats,
    },
    match_world::{FixedMatchLoadout, PlayableBounds, ENGINE_MAX_CHUNK, ENGINE_MIN_CHUNK},
    matchmaking::{FrozenRoster, QueuedPlayer},
    ports::MatchWorldSpec,
};

pub(super) fn match_spec() -> MatchWorldSpec {
    let roster = (0..10)
        .map(|index| QueuedPlayer {
            account_id: Uuid::from_u128(100 + index),
            public_player_id: Uuid::from_u128(200 + index),
            enqueued_at: OffsetDateTime::UNIX_EPOCH,
        })
        .collect::<Vec<_>>();
    MatchWorldSpec {
        match_id: Uuid::from_u128(1),
        world_name: "stage5-test".to_owned(),
        seed: 7,
        generation_version: "generation-v1".to_owned(),
        gameplay_version: "pvp-mvp-v1".to_owned(),
        config_version: "balance-v1".to_owned(),
        roster: FrozenRoster::try_from(roster).unwrap(),
        engine_min_chunk: ENGINE_MIN_CHUNK,
        engine_max_chunk: ENGINE_MAX_CHUNK,
        playable_bounds: PlayableBounds::EXTRACTION,
        loadout: FixedMatchLoadout::default(),
        saving: false,
    }
}

#[test]
fn gameplay_rejects_a_health_loadout_that_disagrees_with_the_protocol() {
    let mut spec = match_spec();
    spec.loadout.max_health_half_hearts = 18;
    let mut world = World::new(&spec.world_name, &WorldConfig::default());

    assert_eq!(
        install_gameplay_runtime(
            &mut world,
            &spec,
            GameplayAuthority::allow_all_at(Duration::ZERO),
        ),
        Err(GameplayInstallError::LoadoutMismatch)
    );
}

fn world_and_player(
    now: Duration,
    initial: Option<ResourceStack>,
) -> (World, specs::Entity, Uuid, Uuid) {
    let spec = match_spec();
    let participant = spec.roster.iter().next().unwrap().clone();
    let mut world = World::new(&spec.world_name, &WorldConfig::default());
    install_gameplay_runtime(&mut world, &spec, GameplayAuthority::allow_all_at(now)).unwrap();
    let mut inventory = MatchInventory::new(64).unwrap();
    if let Some(stack) = initial {
        inventory.insert(stack.resource, stack.quantity).unwrap();
    }
    let entity = world
        .ecs_mut()
        .create_entity()
        .with(MatchPlayerComp::new(
            participant.account_id,
            participant.public_player_id,
            participant.seat_id,
        ))
        .with(ResourceInventoryComp::new(inventory))
        .with(FixedEquipmentComp::standard())
        .with(MiningComp::new())
        .with(HealthComp::new(HealthState::new(20).unwrap()))
        .with(CombatComp::new(CombatState::default()))
        .with(RoundStatsComp::new(RoundStats::new(now)))
        .with(EliminationComp::alive())
        .with(ExtractionComp::default())
        .with(PositionComp::new(0.0, 1.0, 0.0))
        .with(DirectionComp::new(1.0, 0.0, 0.0))
        .build();
    (
        world,
        entity,
        participant.account_id,
        participant.public_player_id,
    )
}

#[test]
fn pending_world_loot_is_spawned_and_auto_picked_without_losing_assets() {
    let (mut world, player, _account_id, _public_player_id) =
        world_and_player(Duration::from_secs(5), None);
    let id = DropId::manual(Uuid::from_u128(1), 2, 9);
    world
        .write_resource::<PendingDropQueue>()
        .enqueue(LootDrop::new(id, [0.5, 1.0, 0.0], ResourceBundle::new(0, 65, 0), None).unwrap())
        .unwrap();

    GameplayRuntimeSystem.run_now(world.ecs());
    world.ecs_mut().maintain();

    let inventories = world.read_component::<ResourceInventoryComp>();
    assert_eq!(
        inventories
            .get(player)
            .unwrap()
            .inventory()
            .quantity(ResourceKey::Gold),
        65
    );
    assert_eq!(
        world.read_resource::<PendingDropQueue>().total_quantity(),
        0
    );
    assert_eq!((&world.read_component::<LootDropComp>()).join().count(), 0);
}

#[test]
fn manual_whole_slot_drop_is_owner_protected_and_stale_sequence_is_idempotent() {
    let (mut world, player, _account_id, public_player_id) = world_and_player(
        Duration::from_secs(5),
        Some(ResourceStack {
            resource: ResourceKey::Diamond,
            quantity: 17,
        }),
    );
    let revision = world
        .read_component::<ResourceInventoryComp>()
        .get(player)
        .unwrap()
        .inventory()
        .revision();
    let queue_intent = |world: &mut World, sequence, expected_inventory_revision| {
        world
            .write_resource::<DropSlotIntentQueue>()
            .push(QueuedDropSlotIntent {
                entity: player,
                client_id: public_player_id.to_string(),
                request_id: Uuid::new_v4(),
                sequence,
                payload: DropSlotPayload {
                    slot: 0,
                    expected_inventory_revision,
                },
            })
            .unwrap();
    };
    queue_intent(&mut world, 7, revision);
    GameplayRuntimeSystem.run_now(world.ecs());
    world.ecs_mut().maintain();

    let inventory_revision = {
        let inventories = world.read_component::<ResourceInventoryComp>();
        let inventory = inventories.get(player).unwrap().inventory();
        assert_eq!(inventory.total_quantity(), 0);
        inventory.revision()
    };
    let total_ground = (&world.read_component::<LootDropComp>())
        .join()
        .map(|loot| loot.drop().contents().total())
        .sum::<u64>();
    assert_eq!(total_ground, 17);

    queue_intent(&mut world, 7, inventory_revision);
    GameplayRuntimeSystem.run_now(world.ecs());
    world.ecs_mut().maintain();
    let total_ground = (&world.read_component::<LootDropComp>())
        .join()
        .map(|loot| loot.drop().contents().total())
        .sum::<u64>();
    assert_eq!(total_ground, 17);
}
