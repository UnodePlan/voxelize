use std::time::Duration;

use specs::{Builder, DispatcherBuilder, ReadExpect, RunNow, System, WorldExt, WriteExpect};
use uuid::Uuid;
use voxelize::{
    Block, Chunk, ChunkOptions, ChunkStatus, ChunkUpdatingSystem, Chunks, DirectionComp,
    PositionComp, Registry, VoxelAccess, World, WorldConfig,
};

use super::{
    authority::GameplayAuthority,
    components::{
        EliminationComp, FixedEquipmentComp, MatchPlayerComp, MiningComp, ResourceInventoryComp,
        RoundStatsComp,
    },
    intents::{MiningIntentQueue, QueuedMiningIntent},
    mining_system::MiningResolutionSystem,
    runtime::{install_gameplay_runtime, GameplayRuntimeContext},
    tests::match_spec,
};
use crate::{
    contracts::{MiningIdleReason, MiningPayload, MiningStateData, ResourceKey},
    gameplay::{drop_queue::PendingDropQueue, inventory::MatchInventory, round_stats::RoundStats},
};

const TARGET: [i32; 3] = [2, 2, 0];
const ORIGIN: [f32; 3] = [0.5, 2.5, 0.5];

fn test_world(now: Duration) -> World {
    let spec = match_spec();
    let config = WorldConfig::new().max_height(64).max_light_level(1).build();
    let mut world = World::new(&spec.world_name, &config);
    install_gameplay_runtime(&mut world, &spec, GameplayAuthority::allow_all_at(now)).unwrap();

    let definitions = {
        let context = world.read_resource::<GameplayRuntimeContext>();
        context
            .manifest
            .resources
            .iter()
            .map(|definition| (definition.key, definition.voxel_id))
            .collect::<Vec<_>>()
    };
    let mut registry = Registry::new();
    for (resource, voxel_id) in definitions {
        registry.register_block(&Block::new(resource.as_str()).id(voxel_id).build());
    }
    world.ecs_mut().insert(registry);
    world
}

fn add_player(
    world: &mut World,
    roster_index: usize,
    direction: [f32; 3],
    inventory: MatchInventory,
) -> (specs::Entity, Uuid) {
    let spec = match_spec();
    let participant = spec.roster.iter().nth(roster_index).unwrap();
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
        .with(RoundStatsComp::new(RoundStats::new(Duration::ZERO)))
        .with(EliminationComp::alive())
        .with(PositionComp::new(ORIGIN[0], ORIGIN[1], ORIGIN[2]))
        .with(DirectionComp::new(direction[0], direction[1], direction[2]))
        .build();
    (entity, participant.public_player_id)
}

fn resource_voxel_id(world: &World, resource: ResourceKey) -> u32 {
    world
        .read_resource::<GameplayRuntimeContext>()
        .manifest
        .resources
        .iter()
        .find(|definition| definition.key == resource)
        .unwrap()
        .voxel_id
}

fn add_target_chunk(world: &mut World, status: ChunkStatus, voxels: &[([i32; 3], u32)]) {
    let options = {
        let config = world.config();
        ChunkOptions {
            size: config.chunk_size,
            max_height: config.max_height,
            sub_chunks: config.sub_chunks,
        }
    };
    let mut chunk = Chunk::new("mining-test-chunk", 0, 0, &options);
    for ([x, y, z], voxel_id) in voxels {
        chunk.set_raw_voxel(*x, *y, *z, *voxel_id);
    }
    chunk.status = status;
    world.chunks_mut().add(chunk);
}

fn add_ready_light_neighbors(world: &mut World) {
    let options = {
        let config = world.config();
        ChunkOptions {
            size: config.chunk_size,
            max_height: config.max_height,
            sub_chunks: config.sub_chunks,
        }
    };
    for cx in -1..=1 {
        for cz in -1..=1 {
            if cx == 0 && cz == 0 {
                continue;
            }
            let mut chunk = Chunk::new("mining-test-neighbor", cx, cz, &options);
            chunk.status = ChunkStatus::Ready;
            world.chunks_mut().add(chunk);
        }
    }
}

fn queue_mining(
    world: &mut World,
    entity: specs::Entity,
    public_player_id: Uuid,
    sequence: u32,
    payload: MiningPayload,
) {
    world
        .write_resource::<MiningIntentQueue>()
        .push(QueuedMiningIntent {
            entity,
            client_id: public_player_id.to_string(),
            request_id: Uuid::new_v4(),
            sequence,
            payload,
        })
        .unwrap();
}

fn run_mining_at(world: &mut World, now: Duration) {
    world.ecs_mut().insert(GameplayAuthority::allow_all_at(now));
    MiningResolutionSystem.run_now(world.ecs());
    world.ecs_mut().maintain();
}

fn idle_reason(world: &World, entity: specs::Entity) -> MiningIdleReason {
    let mining = world.read_component::<MiningComp>();
    match mining.get(entity).unwrap().state().snapshot(None).unwrap() {
        MiningStateData::Idle { reason, .. } => reason,
        MiningStateData::Mining { .. } => panic!("expected idle mining state"),
    }
}

#[test]
fn ready_chunk_completes_at_the_exact_duration_boundary() {
    let mut world = test_world(Duration::ZERO);
    let dirt_id = resource_voxel_id(&world, ResourceKey::Dirt);
    add_target_chunk(&mut world, ChunkStatus::Ready, &[(TARGET, dirt_id)]);
    let inventory = MatchInventory::new(64).unwrap();
    let (player, public_player_id) = add_player(&mut world, 0, [1.0, 0.0, 0.0], inventory);

    queue_mining(
        &mut world,
        player,
        public_player_id,
        1,
        MiningPayload::Start { voxel: TARGET },
    );
    run_mining_at(&mut world, Duration::ZERO);
    queue_mining(
        &mut world,
        player,
        public_player_id,
        2,
        MiningPayload::Maintain {},
    );
    run_mining_at(&mut world, Duration::from_millis(499));
    assert_eq!(
        world
            .read_component::<ResourceInventoryComp>()
            .get(player)
            .unwrap()
            .inventory()
            .quantity(ResourceKey::Dirt),
        0
    );

    queue_mining(
        &mut world,
        player,
        public_player_id,
        3,
        MiningPayload::Maintain {},
    );
    run_mining_at(&mut world, Duration::from_millis(500));

    assert_eq!(
        world
            .read_component::<ResourceInventoryComp>()
            .get(player)
            .unwrap()
            .inventory()
            .quantity(ResourceKey::Dirt),
        1
    );
    assert_eq!(idle_reason(&world, player), MiningIdleReason::Completed);
}

#[test]
fn two_players_mining_one_voxel_produce_only_one_resource() {
    let mut world = test_world(Duration::ZERO);
    let dirt_id = resource_voxel_id(&world, ResourceKey::Dirt);
    add_target_chunk(&mut world, ChunkStatus::Ready, &[(TARGET, dirt_id)]);
    let (first, first_id) = add_player(
        &mut world,
        0,
        [1.0, 0.0, 0.0],
        MatchInventory::new(64).unwrap(),
    );
    let (second, second_id) = add_player(
        &mut world,
        1,
        [1.0, 0.0, 0.0],
        MatchInventory::new(64).unwrap(),
    );

    queue_mining(
        &mut world,
        first,
        first_id,
        1,
        MiningPayload::Start { voxel: TARGET },
    );
    queue_mining(
        &mut world,
        second,
        second_id,
        1,
        MiningPayload::Start { voxel: TARGET },
    );
    run_mining_at(&mut world, Duration::ZERO);
    queue_mining(&mut world, first, first_id, 2, MiningPayload::Maintain {});
    queue_mining(&mut world, second, second_id, 2, MiningPayload::Maintain {});
    run_mining_at(&mut world, Duration::from_millis(500));

    let inventories = world.read_component::<ResourceInventoryComp>();
    let first_quantity = inventories
        .get(first)
        .unwrap()
        .inventory()
        .quantity(ResourceKey::Dirt);
    let second_quantity = inventories
        .get(second)
        .unwrap()
        .inventory()
        .quantity(ResourceKey::Dirt);
    assert_eq!(first_quantity + second_quantity, 1);
    assert_eq!(first_quantity, 1, "lower seat id wins an exact tie");
    drop(inventories);
    assert_eq!(idle_reason(&world, second), MiningIdleReason::InvalidBlock);
    assert_eq!(
        world.read_resource::<PendingDropQueue>().total_quantity(),
        0
    );
}

#[derive(Default)]
struct StagingObservation {
    saw_pending_and_unapplied_air: bool,
}

struct ObserveMiningStaging {
    target: [i32; 3],
    original_voxel_id: u32,
}

impl<'a> System<'a> for ObserveMiningStaging {
    type SystemData = (
        ReadExpect<'a, Chunks>,
        ReadExpect<'a, PendingDropQueue>,
        WriteExpect<'a, StagingObservation>,
    );

    fn run(&mut self, (chunks, pending, mut observation): Self::SystemData) {
        let [x, y, z] = self.target;
        observation.saw_pending_and_unapplied_air =
            pending.total_quantity() == 1 && chunks.get_voxel(x, y, z) == self.original_voxel_id;
    }
}

#[test]
fn full_inventory_stages_air_and_chunk_updating_consumes_it_in_one_dispatch() {
    let mut world = test_world(Duration::ZERO);
    let dirt_id = resource_voxel_id(&world, ResourceKey::Dirt);
    add_target_chunk(&mut world, ChunkStatus::Ready, &[(TARGET, dirt_id)]);
    add_ready_light_neighbors(&mut world);
    let mut inventory = MatchInventory::new(1).unwrap();
    for resource in ResourceKey::ALL {
        inventory.insert(resource, 4).unwrap();
    }
    let (player, public_player_id) = add_player(&mut world, 0, [1.0, 0.0, 0.0], inventory);
    queue_mining(
        &mut world,
        player,
        public_player_id,
        1,
        MiningPayload::Start { voxel: TARGET },
    );
    run_mining_at(&mut world, Duration::ZERO);
    queue_mining(
        &mut world,
        player,
        public_player_id,
        2,
        MiningPayload::Maintain {},
    );
    world
        .ecs_mut()
        .insert(GameplayAuthority::allow_all_at(Duration::from_millis(500)));
    world.ecs_mut().insert(StagingObservation::default());

    let mut dispatcher = DispatcherBuilder::new()
        .with(MiningResolutionSystem, "test-mining-resolution", &[])
        .with(
            ObserveMiningStaging {
                target: TARGET,
                original_voxel_id: dirt_id,
            },
            "test-observe-mining-staging",
            &["test-mining-resolution"],
        )
        .with(
            ChunkUpdatingSystem,
            "test-chunk-updating",
            &["test-observe-mining-staging"],
        )
        .build();
    dispatcher.setup(world.ecs_mut());
    dispatcher.dispatch(world.ecs());
    world.ecs_mut().maintain();

    assert!(
        world
            .read_resource::<StagingObservation>()
            .saw_pending_and_unapplied_air
    );
    assert_eq!(
        world.read_resource::<PendingDropQueue>().total_quantity(),
        1
    );
    assert_eq!(world.chunks().get_voxel(TARGET[0], TARGET[1], TARGET[2]), 0);
}

fn rejected_start_reason(
    status: ChunkStatus,
    direction: [f32; 3],
    add_occluder: bool,
) -> MiningIdleReason {
    let mut world = test_world(Duration::ZERO);
    let dirt_id = resource_voxel_id(&world, ResourceKey::Dirt);
    let mut voxels = vec![(TARGET, dirt_id)];
    if add_occluder {
        voxels.push(([1, 2, 0], dirt_id));
    }
    add_target_chunk(&mut world, status, &voxels);
    let (player, public_player_id) =
        add_player(&mut world, 0, direction, MatchInventory::new(64).unwrap());
    queue_mining(
        &mut world,
        player,
        public_player_id,
        1,
        MiningPayload::Start { voxel: TARGET },
    );
    run_mining_at(&mut world, Duration::ZERO);
    idle_reason(&world, player)
}

#[test]
fn mining_rejects_occluded_unready_and_invalid_directions() {
    assert_eq!(
        rejected_start_reason(ChunkStatus::Ready, [1.0, 0.0, 0.0], true),
        MiningIdleReason::Occluded
    );
    assert_eq!(
        rejected_start_reason(ChunkStatus::Generating(0), [1.0, 0.0, 0.0], false),
        MiningIdleReason::InvalidBlock
    );
    assert_eq!(
        rejected_start_reason(ChunkStatus::Ready, [0.0, 0.0, 0.0], false),
        MiningIdleReason::Disconnected
    );
    assert_eq!(
        rejected_start_reason(ChunkStatus::Ready, [f32::NAN, 0.0, 0.0], false),
        MiningIdleReason::Disconnected
    );
}
