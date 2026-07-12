use std::{
    collections::{BTreeMap, HashSet, VecDeque},
    sync::Arc,
};

use sha2::{Digest, Sha256};
use voxelize::{Chunk, ChunkOptions, ChunkStage, Resources, VoxelAccess, WorldConfig};

use super::{stage::ExtractionTerrainStage, GenerationPlan, MapPoint};
use crate::{
    contracts::bundled_manifest, engine_catalog::EngineCatalog, match_world::engine_seed_v1,
};

const FIXED_SEED: u64 = 0x1122_3344_5566_7788;

fn plan(seed: u64) -> (Arc<GenerationPlan>, EngineCatalog) {
    let manifest = bundled_manifest().unwrap();
    let catalog = EngineCatalog::from_manifest(&manifest).unwrap();
    let plan = GenerationPlan::new(
        seed,
        &manifest.generation_version,
        &manifest.config_version,
        catalog.resources(),
    )
    .unwrap();
    (Arc::new(plan), catalog)
}

fn world_config(plan: &GenerationPlan) -> WorldConfig {
    WorldConfig::new()
        .min_chunk([-10, -10])
        .max_chunk([9, 9])
        .max_height(plan.config().max_height)
        .water_level(0)
        .build()
}

fn process_chunk(
    plan: Arc<GenerationPlan>,
    catalog: &EngineCatalog,
    config: &WorldConfig,
    cx: i32,
    cz: i32,
) -> Chunk {
    let options = ChunkOptions {
        size: config.chunk_size,
        max_height: config.max_height,
        sub_chunks: config.sub_chunks,
    };
    ExtractionTerrainStage::new(plan).process(
        Chunk::new("generation-test", cx, cz, &options),
        Resources {
            registry: catalog.blocks(),
            config,
        },
        None,
    )
}

fn fingerprint(seed: u64, mut chunk_order: Vec<(i32, i32)>) -> String {
    let (plan, catalog) = plan(seed);
    let config = world_config(&plan);
    let mut chunk_digests = BTreeMap::new();
    for (cx, cz) in chunk_order.drain(..) {
        let chunk = process_chunk(plan.clone(), &catalog, &config, cx, cz);
        let mut digest = Sha256::new();
        let min_x = chunk.min.0.max(plan.config().min_xz);
        let max_x = chunk.max.0.min(plan.config().max_xz_exclusive);
        let min_z = chunk.min.2.max(plan.config().min_xz);
        let max_z = chunk.max.2.min(plan.config().max_xz_exclusive);
        for x in min_x..max_x {
            for z in min_z..max_z {
                for y in 0..plan.config().max_height as i32 {
                    digest.update(chunk.get_voxel(x, y, z).to_le_bytes());
                }
            }
        }
        chunk_digests.insert((cx, cz), digest.finalize());
    }

    let mut digest = Sha256::new();
    for ((cx, cz), chunk_digest) in chunk_digests {
        digest.update(cx.to_le_bytes());
        digest.update(cz.to_le_bytes());
        digest.update(chunk_digest);
    }
    hex::encode(digest.finalize())
}

fn all_chunk_coords() -> Vec<(i32, i32)> {
    (-10..=9)
        .flat_map(|cx| (-10..=9).map(move |cz| (cx, cz)))
        .collect()
}

#[test]
fn fixed_version_has_stable_actual_stage_fingerprint() {
    let fingerprint = fingerprint(FIXED_SEED, all_chunk_coords());
    assert_eq!(
        fingerprint,
        "234903c7af2917afb0e3b9aa643f5848c40f8e12b5494cd2b4d18187bb881df5"
    );
}

#[test]
fn generation_is_independent_of_chunk_order_and_uses_all_seed_bits() {
    let forward = all_chunk_coords();
    let mut reverse = forward.clone();
    reverse.reverse();
    assert_eq!(
        fingerprint(FIXED_SEED, forward),
        fingerprint(FIXED_SEED, reverse)
    );

    let low_seed = 1_u64;
    let high_seed = 1_u64 << 32;
    assert_eq!(engine_seed_v1(low_seed), engine_seed_v1(high_seed));
    assert_ne!(
        fingerprint(low_seed, all_chunk_coords()),
        fingerprint(high_seed, all_chunk_coords())
    );
}

#[test]
fn a_fresh_match_does_not_inherit_mutated_chunk_state() {
    let (first_plan, first_catalog) = plan(FIXED_SEED);
    let first_config = world_config(&first_plan);
    let mut first_chunk = process_chunk(first_plan, &first_catalog, &first_config, 0, 0);
    assert_ne!(first_chunk.get_voxel(0, 48, 0), 0);
    first_chunk.set_voxel(0, 48, 0, 0);
    assert_eq!(first_chunk.get_voxel(0, 48, 0), 0);

    let (fresh_plan, fresh_catalog) = plan(FIXED_SEED);
    let fresh_config = world_config(&fresh_plan);
    let fresh_chunk = process_chunk(fresh_plan, &fresh_catalog, &fresh_config, 0, 0);
    assert_ne!(fresh_chunk.get_voxel(0, 48, 0), 0);
}

#[test]
fn resource_counts_depth_and_components_follow_v1_balance() {
    let (plan, catalog) = plan(FIXED_SEED);
    let config = world_config(&plan);
    let resources = plan.resources();
    let mut dirt = 0_usize;
    let mut dirt_surface_columns = 0_usize;
    let mut gold = HashSet::new();
    let mut diamond = HashSet::new();

    for (cx, cz) in all_chunk_coords() {
        let chunk = process_chunk(plan.clone(), &catalog, &config, cx, cz);
        let min_x = chunk.min.0.max(plan.config().min_xz);
        let max_x = chunk.max.0.min(plan.config().max_xz_exclusive);
        let min_z = chunk.min.2.max(plan.config().min_xz);
        let max_z = chunk.max.2.min(plan.config().max_xz_exclusive);
        for x in min_x..max_x {
            for z in min_z..max_z {
                for y in 0..=plan.config().surface_y {
                    match chunk.get_voxel(x, y, z) {
                        id if id == resources.dirt.voxel_id => {
                            dirt += 1;
                            if y == plan.config().surface_y {
                                dirt_surface_columns += 1;
                            }
                        }
                        id if id == resources.gold.voxel_id => {
                            gold.insert((x, y, z));
                        }
                        id if id == resources.diamond.voxel_id => {
                            diamond.insert((x, y, z));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    assert_eq!(dirt, 4_387_170);
    assert_eq!(dirt_surface_columns, 90_000);
    assert_eq!(gold.len(), 20_265);
    assert_eq!(diamond.len(), 2_565);
    assert_eq!(connected_component_sizes(&gold), vec![4_053; 5]);
    assert_eq!(connected_component_sizes(&diamond), vec![855; 3]);
    assert_depth_range(&gold, 22, 39);
    assert_depth_range(&diamond, 7, 17);
    assert!(mean_center_distance_squared(&diamond) < mean_center_distance_squared(&gold));
}

#[test]
fn spawn_and_extraction_candidates_are_valid_and_fair() {
    let (plan, _) = plan(FIXED_SEED);
    let spawns = plan.layout().spawn_points();
    for point in spawns {
        assert!(plan.config().contains_xz(point.x, point.z));
        assert_eq!(point.y, plan.config().surface_y + 2);
    }
    for left in 0..spawns.len() {
        for right in left + 1..spawns.len() {
            assert!(horizontal_distance_squared(spawns[left], spawns[right]) >= 5_000);
        }
    }

    let nearest_gold = spawns
        .iter()
        .map(|spawn| {
            plan.gold_deposits()
                .iter()
                .map(|deposit| horizontal_distance_squared(*spawn, deposit.center))
                .min()
                .unwrap()
        })
        .collect::<Vec<_>>();
    let nearest_min = *nearest_gold.iter().min().unwrap();
    let nearest_max = *nearest_gold.iter().max().unwrap();
    assert!(nearest_max - nearest_min <= 2_500);

    for point in plan.layout().extraction_candidates() {
        assert!(plan.config().contains_xz(point.x, point.z));
        assert!((242..=256).contains(&(point.x * point.x + point.z * point.z)));
    }

    let selected = (0..128_u64)
        .map(|seed| self::plan(seed).0.layout().selected_extraction())
        .collect::<HashSet<_>>();
    assert_eq!(selected.len(), 8);
}

#[test]
fn stage_stays_inside_chunks_and_floor_is_not_initially_mineable() {
    let (plan, catalog) = plan(FIXED_SEED);
    let config = world_config(&plan);
    for (cx, cz) in [(-10, -10), (-1, 0), (0, -1), (9, 9)] {
        let chunk = process_chunk(plan.clone(), &catalog, &config, cx, cz);
        assert!(chunk.extra_changes.is_empty());
        for x in chunk.min.0..chunk.max.0 {
            for z in chunk.min.2..chunk.max.2 {
                for y in 0..=plan.config().surface_y {
                    assert_eq!(
                        chunk.get_voxel(x, y, z),
                        plan.voxel_at(MapPoint::new(x, y, z))
                    );
                }
            }
        }
    }
    assert!(!plan.is_initially_mineable(MapPoint::new(140, 0, 140)));
    assert!(plan.is_initially_mineable(MapPoint::new(140, 1, 140)));
}

fn connected_component_sizes(voxels: &HashSet<(i32, i32, i32)>) -> Vec<usize> {
    let mut visited = HashSet::new();
    let mut sizes = Vec::new();
    for &start in voxels {
        if !visited.insert(start) {
            continue;
        }
        let mut queue = VecDeque::from([start]);
        let mut size = 0;
        while let Some((x, y, z)) = queue.pop_front() {
            size += 1;
            for neighbor in [
                (x - 1, y, z),
                (x + 1, y, z),
                (x, y - 1, z),
                (x, y + 1, z),
                (x, y, z - 1),
                (x, y, z + 1),
            ] {
                if voxels.contains(&neighbor) && visited.insert(neighbor) {
                    queue.push_back(neighbor);
                }
            }
        }
        sizes.push(size);
    }
    sizes.sort_unstable();
    sizes
}

fn assert_depth_range(voxels: &HashSet<(i32, i32, i32)>, min_y: i32, max_y: i32) {
    let actual_min = voxels.iter().map(|(_, y, _)| *y).min().unwrap();
    let actual_max = voxels.iter().map(|(_, y, _)| *y).max().unwrap();
    assert!(actual_min >= min_y, "最低深度 {actual_min} 超出配置");
    assert!(actual_max <= max_y, "最高深度 {actual_max} 超出配置");
}

fn mean_center_distance_squared(voxels: &HashSet<(i32, i32, i32)>) -> f64 {
    voxels
        .iter()
        .map(|(x, _, z)| f64::from(x * x + z * z))
        .sum::<f64>()
        / voxels.len() as f64
}

fn horizontal_distance_squared(left: MapPoint, right: MapPoint) -> i32 {
    let dx = left.x - right.x;
    let dz = left.z - right.z;
    dx * dx + dz * dz
}
