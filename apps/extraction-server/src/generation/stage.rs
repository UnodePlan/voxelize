use std::sync::Arc;

use voxelize::{Chunk, ChunkStage, Resources, Space, Vec3, VoxelAccess, World};

use super::{GenerationPlan, MapPoint};

pub(crate) fn install_generation_stage(world: &mut World, plan: Arc<GenerationPlan>) {
    world
        .pipeline_mut()
        .add_stage(ExtractionTerrainStage::new(plan.clone()));
    world.ecs_mut().insert(plan);
}

pub(super) struct ExtractionTerrainStage {
    plan: Arc<GenerationPlan>,
}

impl ExtractionTerrainStage {
    pub(super) fn new(plan: Arc<GenerationPlan>) -> Self {
        Self { plan }
    }
}

impl ChunkStage for ExtractionTerrainStage {
    fn name(&self) -> String {
        format!(
            "ExtractionTerrain({}/{})",
            self.plan.config().generation_version,
            self.plan.config().config_version
        )
    }

    fn process(&self, mut chunk: Chunk, _: Resources, _: Option<Space>) -> Chunk {
        let Vec3(min_x, _, min_z) = chunk.min;
        let Vec3(max_x, _, max_z) = chunk.max;
        for x in min_x..max_x {
            for z in min_z..max_z {
                for y in 0..=self.plan.config().surface_y {
                    let voxel = self.plan.voxel_at(MapPoint::new(x, y, z));
                    if voxel != 0 {
                        chunk.set_voxel(x, y, z, voxel);
                    }
                }
            }
        }
        chunk
    }
}
