use voxelize::{ChunkProjection, ChunkProjectionError, World};

use crate::engine_catalog::MatchResourceCatalog;

pub(crate) fn install_anti_xray(
    world: &mut World,
    resources: MatchResourceCatalog,
) -> Result<(), ChunkProjectionError> {
    let projection = ChunkProjection::obfuscating([
        (resources.gold.voxel_id, resources.dirt.voxel_id),
        (resources.diamond.voxel_id, resources.dirt.voxel_id),
    ])?;
    world.ecs_mut().insert(projection);
    Ok(())
}
