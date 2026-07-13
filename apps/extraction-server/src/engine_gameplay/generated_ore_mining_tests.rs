use std::{sync::Arc, time::Duration};

use specs::{DispatcherBuilder, WorldExt};
use voxelize::{Chunk, ChunkUpdatingSystem, ChunkUtils, Vec2, VoxelAccess, World, WorldConfig};

use super::{
    authority::GameplayAuthority,
    components::ResourceInventoryComp,
    mining_system::MiningResolutionSystem,
    mining_tests::{add_player_at, queue_mining, resource_voxel_id, test_world_with_seed},
};
use crate::{
    contracts::{MiningPayload, ResourceKey},
    engine_catalog::EngineCatalog,
    gameplay::{drop_queue::PendingDropQueue, inventory::MatchInventory},
    generation::{
        test_support::{fixture, ready_chunk, FIXED_SEED},
        GenerationPlan, MapPoint,
    },
};

struct GeneratedOre {
    target: MapPoint,
    chunk: Chunk,
    ore_id: u32,
    dirt_id: u32,
}

fn find_generated_ore(
    plan: &Arc<GenerationPlan>,
    catalog: &EngineCatalog,
    config: &WorldConfig,
    ore_id: u32,
    dirt_id: u32,
) -> GeneratedOre {
    let mut chunk_coords = (-10..=9)
        .flat_map(|cx| (-10..=9).map(move |cz| Vec2(cx, cz)))
        .collect::<Vec<_>>();
    chunk_coords.sort_by_key(|coords| coords.0 * coords.0 + coords.1 * coords.1);

    for coords in chunk_coords {
        let chunk = ready_chunk(plan.clone(), catalog, config, coords);
        let min_x = chunk.min.0.max(plan.config().min_xz) + 1;
        let max_x = chunk.max.0.min(plan.config().max_xz_exclusive);
        let min_z = chunk.min.2.max(plan.config().min_xz);
        let max_z = chunk.max.2.min(plan.config().max_xz_exclusive);
        for x in min_x..max_x {
            for z in min_z..max_z {
                for y in 1..plan.config().surface_y {
                    if chunk.get_voxel(x, y, z) == ore_id && chunk.get_voxel(x - 1, y, z) == dirt_id
                    {
                        return GeneratedOre {
                            target: MapPoint::new(x, y, z),
                            chunk,
                            ore_id,
                            dirt_id,
                        };
                    }
                }
            }
        }
    }
    panic!("固定生成种子必须包含可从真实 Chunk 定位的目标矿石");
}

fn install_generated_neighborhood(
    world: &mut World,
    plan: &Arc<GenerationPlan>,
    catalog: &EngineCatalog,
    config: &WorldConfig,
    ore: GeneratedOre,
) {
    let GeneratedOre {
        target,
        mut chunk,
        ore_id,
        dirt_id,
    } = ore;
    let target_coords =
        ChunkUtils::map_voxel_to_chunk(target.x, target.y, target.z, config.chunk_size);
    assert_eq!(chunk.get_voxel(target.x, target.y, target.z), ore_id);
    assert_eq!(chunk.get_voxel(target.x - 1, target.y, target.z), dirt_id);
    assert!(chunk.extra_changes.is_empty());

    // 只清掉真实生成的相邻泥土来模拟已挖通道，目标矿石保持 ChunkStage 原始输出。
    chunk.set_raw_voxel(target.x - 1, target.y, target.z, 0);
    world.chunks_mut().add(chunk);
    for cx in target_coords.0 - 1..=target_coords.0 + 1 {
        for cz in target_coords.1 - 1..=target_coords.1 + 1 {
            let coords = Vec2(cx, cz);
            if coords != target_coords {
                world
                    .chunks_mut()
                    .add(ready_chunk(plan.clone(), catalog, config, coords));
            }
        }
    }
}

fn run_mining_and_chunk_update(world: &mut World, now: Duration) {
    world.ecs_mut().insert(GameplayAuthority::allow_all_at(now));
    let mut dispatcher = DispatcherBuilder::new()
        .with(MiningResolutionSystem, "generated-ore-mining", &[])
        .with(
            ChunkUpdatingSystem,
            "generated-ore-chunk-updating",
            &["generated-ore-mining"],
        )
        .build();
    dispatcher.setup(world.ecs_mut());
    dispatcher.dispatch(world.ecs());
    world.ecs_mut().maintain();
}

fn inventory_quantity(world: &World, player: specs::Entity, resource: ResourceKey) -> u32 {
    world
        .read_component::<ResourceInventoryComp>()
        .get(player)
        .unwrap()
        .inventory()
        .quantity(resource)
}

fn assert_generated_ore_mines_once(resource: ResourceKey, required_ms: u64) {
    let (plan, catalog, generation_config) = fixture();
    assert_eq!(plan.seed(), FIXED_SEED);
    assert_eq!(plan.config().generation_version, "generation-v1");
    assert_eq!(plan.config().config_version, "balance-v1");

    let mut world = test_world_with_seed(Duration::ZERO, FIXED_SEED);
    let ore_id = resource_voxel_id(&world, resource);
    let dirt_id = resource_voxel_id(&world, ResourceKey::Dirt);
    let ore = find_generated_ore(&plan, &catalog, &generation_config, ore_id, dirt_id);
    let target = ore.target;
    install_generated_neighborhood(&mut world, &plan, &catalog, &generation_config, ore);
    let origin = [
        target.x as f32 - 0.5,
        target.y as f32 + 0.5,
        target.z as f32 + 0.5,
    ];
    let (player, public_player_id) = add_player_at(
        &mut world,
        0,
        origin,
        [1.0, 0.0, 0.0],
        MatchInventory::new(64).unwrap(),
    );
    let target_voxel = [target.x, target.y, target.z];
    queue_mining(
        &mut world,
        player,
        public_player_id,
        1,
        MiningPayload::Start {
            voxel: target_voxel,
        },
    );
    run_mining_and_chunk_update(&mut world, Duration::ZERO);

    let mut sequence = 2;
    let mut elapsed_ms = 300;
    while elapsed_ms < required_ms - 1 {
        queue_mining(
            &mut world,
            player,
            public_player_id,
            sequence,
            MiningPayload::Maintain {},
        );
        run_mining_and_chunk_update(&mut world, Duration::from_millis(elapsed_ms));
        sequence += 1;
        elapsed_ms += 300;
    }
    queue_mining(
        &mut world,
        player,
        public_player_id,
        sequence,
        MiningPayload::Maintain {},
    );
    run_mining_and_chunk_update(&mut world, Duration::from_millis(required_ms - 1));
    sequence += 1;
    assert_eq!(inventory_quantity(&world, player, resource), 0);
    assert_eq!(
        world.chunks().get_voxel(target.x, target.y, target.z),
        ore_id
    );

    queue_mining(
        &mut world,
        player,
        public_player_id,
        sequence,
        MiningPayload::Maintain {},
    );
    run_mining_and_chunk_update(&mut world, Duration::from_millis(required_ms));
    sequence += 1;
    assert_eq!(inventory_quantity(&world, player, resource), 1);
    assert_eq!(world.chunks().get_voxel(target.x, target.y, target.z), 0);
    assert_eq!(
        world.read_resource::<PendingDropQueue>().total_quantity(),
        0
    );

    queue_mining(
        &mut world,
        player,
        public_player_id,
        sequence,
        MiningPayload::Start {
            voxel: target_voxel,
        },
    );
    run_mining_and_chunk_update(&mut world, Duration::from_millis(required_ms + 1));
    assert_eq!(inventory_quantity(&world, player, resource), 1);
    assert_eq!(world.chunks().get_voxel(target.x, target.y, target.z), 0);
}

#[test]
fn fixed_v1_generated_gold_requires_1500ms_and_mines_once() {
    assert_generated_ore_mines_once(ResourceKey::Gold, 1_500);
}

#[test]
fn fixed_v1_generated_diamond_requires_3000ms_and_mines_once() {
    assert_generated_ore_mines_once(ResourceKey::Diamond, 3_000);
}
