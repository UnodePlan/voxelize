use voxelize::{
    BlockUtils, ChunkProjection, ChunkUtils, Chunks, Vec3, VoxelAccess, World, WorldConfig,
};

use super::{
    test_support::{fixture, ready_chunk},
    GenerationPlan, MapPoint,
};
use crate::{engine_anti_xray::install_anti_xray, engine_catalog::EngineCatalog};

fn find_ore(plan: &GenerationPlan, ore_id: u32, on_positive_x_edge: bool) -> MapPoint {
    for x in plan.config().min_xz..plan.config().max_xz_exclusive {
        if on_positive_x_edge && x.rem_euclid(16) != 15 {
            continue;
        }
        if on_positive_x_edge && x + 1 >= plan.config().max_xz_exclusive {
            continue;
        }
        for z in plan.config().min_xz..plan.config().max_xz_exclusive {
            if !on_positive_x_edge && matches!(x.rem_euclid(16), 0 | 15) {
                continue;
            }
            if !on_positive_x_edge && matches!(z.rem_euclid(16), 0 | 15) {
                continue;
            }
            for y in 1..plan.config().surface_y {
                let point = MapPoint::new(x, y, z);
                if plan.voxel_at(point) != ore_id {
                    continue;
                }
                let enclosed = [
                    [1, 0, 0],
                    [-1, 0, 0],
                    [0, 1, 0],
                    [0, -1, 0],
                    [0, 0, 1],
                    [0, 0, -1],
                ]
                .into_iter()
                .all(|[ox, oy, oz]| plan.voxel_at(MapPoint::new(x + ox, y + oy, z + oz)) != 0);
                if enclosed {
                    return point;
                }
            }
        }
    }
    panic!("fixed generation seed must contain a matching enclosed ore voxel");
}

fn installed_projection(config: &WorldConfig, catalog: &EngineCatalog) -> ChunkProjection {
    let mut world = World::new("anti-xray-install", config);
    install_anti_xray(&mut world, catalog.resources()).unwrap();
    let projection = (*world.read_resource::<ChunkProjection>()).clone();
    projection
}

fn model_id(model: &voxelize::ChunkProtocol, point: MapPoint, chunk_size: usize) -> u32 {
    let local_x = point.x.rem_euclid(chunk_size as i32) as usize;
    let local_z = point.z.rem_euclid(chunk_size as i32) as usize;
    BlockUtils::extract_id(model.voxels.as_ref().unwrap()[&[local_x, point.y as usize, local_z]])
}

#[test]
fn fixed_seed_ready_chunk_load_hides_enclosed_ore_without_changing_truth() {
    let (plan, catalog, config) = fixture();
    let resources = catalog.resources();
    for ore_id in [resources.gold.voxel_id, resources.diamond.voxel_id] {
        let ore = find_ore(&plan, ore_id, false);
        let coords = ChunkUtils::map_voxel_to_chunk(ore.x, ore.y, ore.z, config.chunk_size);
        let mut chunks = Chunks::new(&config);
        chunks.add(ready_chunk(plan.clone(), &catalog, &config, coords.clone()));
        let mut projection = installed_projection(&config, &catalog);

        let model = projection.project_chunk(
            chunks.get(&coords).unwrap(),
            &chunks,
            catalog.blocks(),
            false,
            0..config.sub_chunks as u32,
        );
        let surface = MapPoint::new(ore.x, plan.config().surface_y, ore.z);

        assert_eq!(
            model_id(&model, ore, config.chunk_size),
            resources.dirt.voxel_id
        );
        assert_eq!(chunks.get_voxel(ore.x, ore.y, ore.z), ore_id);
        assert_eq!(
            model_id(&model, surface, config.chunk_size),
            resources.dirt.voxel_id
        );
        assert!(!model.voxels.unwrap().data.iter().any(|raw| {
            matches!(
                BlockUtils::extract_id(*raw),
                id if id == resources.gold.voxel_id || id == resources.diamond.voxel_id
            )
        }));
    }
}

#[test]
fn opened_cross_chunk_ore_is_visible_to_current_and_late_loads() {
    let (plan, catalog, config) = fixture();
    let resources = catalog.resources();
    let ore = find_ore(&plan, resources.diamond.voxel_id, true);
    let ore_coords = ChunkUtils::map_voxel_to_chunk(ore.x, ore.y, ore.z, config.chunk_size);
    let air = Vec3(ore.x + 1, ore.y, ore.z);
    let air_coords = ChunkUtils::map_voxel_to_chunk(air.0, air.1, air.2, config.chunk_size);
    assert_ne!(ore_coords, air_coords);
    let mut chunks = Chunks::new(&config);
    chunks.add(ready_chunk(
        plan.clone(),
        &catalog,
        &config,
        ore_coords.clone(),
    ));
    chunks.add(ready_chunk(plan, &catalog, &config, air_coords));
    chunks.set_voxel(air.0, air.1, air.2, 0);
    let mut projection = installed_projection(&config, &catalog);

    let first = projection.project_chunk(
        chunks.get(&ore_coords).unwrap(),
        &chunks,
        catalog.blocks(),
        false,
        0..config.sub_chunks as u32,
    );
    let late = projection.project_chunk(
        chunks.get(&ore_coords).unwrap(),
        &chunks,
        catalog.blocks(),
        false,
        0..config.sub_chunks as u32,
    );

    assert_eq!(
        model_id(&first, ore, config.chunk_size),
        resources.diamond.voxel_id
    );
    assert_eq!(
        model_id(&late, ore, config.chunk_size),
        resources.diamond.voxel_id
    );
    assert_eq!(
        chunks.get_voxel(ore.x, ore.y, ore.z),
        resources.diamond.voxel_id
    );
}
