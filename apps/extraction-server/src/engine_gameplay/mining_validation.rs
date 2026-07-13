use std::cell::Cell;

use voxelize::{trace, ChunkUtils, Chunks, DirectionComp, PositionComp, Vec2, Vec3, VoxelAccess};

use super::{components::FixedEquipmentComp, runtime::GameplayRuntimeContext};
use crate::{
    contracts::{ErrorCode, MiningIdleReason},
    gameplay::{
        harvest::HarvestedVoxelSet,
        mining::{MiningTarget, VoxelCoordinate},
    },
};

pub(super) struct MiningValidationAccess<'a> {
    pub context: &'a GameplayRuntimeContext,
    pub chunks: &'a Chunks,
    pub harvested: &'a HarvestedVoxelSet,
    pub equipment: &'a FixedEquipmentComp,
    pub position: &'a PositionComp,
    pub direction: &'a DirectionComp,
}

pub(super) fn validate_mining_target(
    requested: VoxelCoordinate,
    access: MiningValidationAccess<'_>,
) -> Result<MiningTarget, MiningValidationError> {
    if !access.equipment.has_basic_pickaxe() {
        return Err(MiningValidationError::InvalidState);
    }
    validate_requested_voxel(requested, access.context, access.chunks)?;
    if access.harvested.contains(requested) {
        return Err(MiningValidationError::InvalidBlock);
    }

    let mut origin = access.position.0.clone();
    origin.1 += access.context.config.mining_eye_offset;
    let mut direction = access.direction.0.clone();
    if ![
        origin.0,
        origin.1,
        origin.2,
        direction.0,
        direction.1,
        direction.2,
    ]
    .into_iter()
    .all(f32::is_finite)
        || !access
            .context
            .playable_bounds
            .contains_xz(origin.0, origin.2)
        || origin.1 < 0.0
        || origin.1 >= access.context.max_height as f32
    {
        return Err(MiningValidationError::InvalidState);
    }
    let direction_length_squared =
        direction.0 * direction.0 + direction.1 * direction.1 + direction.2 * direction.2;
    if !direction_length_squared.is_finite() || direction_length_squared <= f32::EPSILON {
        return Err(MiningValidationError::InvalidState);
    }

    let first_hit = Cell::new(None);
    let unavailable = Cell::new(false);
    // Chunks 会把缺失区块读成 Air；权威射线必须单独记录并对缺失数据失败关闭。
    let get_voxel = |x: i32, y: i32, z: i32| {
        if y < 0 || y >= access.context.max_height {
            unavailable.set(true);
            return true;
        }
        let coords = ChunkUtils::map_voxel_to_chunk(x, y, z, access.context.chunk_size);
        if !access.chunks.is_chunk_ready(&coords) {
            unavailable.set(true);
            return true;
        }
        if access.chunks.get_voxel(x, y, z) != 0 {
            first_hit.set(Some(VoxelCoordinate::new(x, y, z)));
            true
        } else {
            false
        }
    };
    let mut hit_position = Vec3::default();
    let mut hit_normal = Vec3::default();
    let hit = trace(
        access.context.config.mining_reach,
        &get_voxel,
        &mut origin,
        &mut direction,
        &mut hit_position,
        &mut hit_normal,
    );
    if unavailable.get() {
        return Err(MiningValidationError::ChunkUnavailable);
    }
    if !hit {
        return Err(MiningValidationError::OutOfRange);
    }
    if first_hit.get() != Some(requested) {
        return Err(MiningValidationError::Occluded);
    }

    let voxel_id = access
        .chunks
        .get_voxel(requested.x, requested.y, requested.z);
    let resource = access
        .context
        .resource_for_voxel(voxel_id)
        .ok_or(MiningValidationError::InvalidBlock)?;
    Ok(MiningTarget {
        voxel: requested,
        voxel_id,
        resource,
    })
}

fn validate_requested_voxel(
    requested: VoxelCoordinate,
    context: &GameplayRuntimeContext,
    chunks: &Chunks,
) -> Result<(), MiningValidationError> {
    if !context
        .playable_bounds
        .contains_xz(requested.x as f32, requested.z as f32)
        || requested.y < context.min_mineable_y
        || requested.y >= context.max_height
    {
        return Err(MiningValidationError::OutOfRange);
    }
    let coords: Vec2<i32> =
        ChunkUtils::map_voxel_to_chunk(requested.x, requested.y, requested.z, context.chunk_size);
    if !chunks.is_chunk_ready(&coords) {
        return Err(MiningValidationError::ChunkUnavailable);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MiningValidationError {
    InvalidState,
    OutOfRange,
    Occluded,
    InvalidBlock,
    ChunkUnavailable,
}

impl MiningValidationError {
    pub(super) const fn protocol(self) -> (MiningIdleReason, ErrorCode, bool) {
        match self {
            Self::InvalidState => (
                MiningIdleReason::Disconnected,
                ErrorCode::GameInvalidState,
                false,
            ),
            Self::OutOfRange => (
                MiningIdleReason::OutOfRange,
                ErrorCode::GameOutOfRange,
                false,
            ),
            Self::Occluded => (MiningIdleReason::Occluded, ErrorCode::GameOutOfRange, false),
            Self::InvalidBlock => (
                MiningIdleReason::InvalidBlock,
                ErrorCode::GameInvalidState,
                false,
            ),
            Self::ChunkUnavailable => (
                MiningIdleReason::InvalidBlock,
                ErrorCode::ServiceUnavailable,
                true,
            ),
        }
    }
}
