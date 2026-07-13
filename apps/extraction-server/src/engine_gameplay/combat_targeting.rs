use std::cell::Cell;

use specs::Entity;
use voxelize::{trace, ChunkUtils, Chunks, Registry, Vec3, VoxelAccess};

use crate::{
    gameplay::{
        combat::BASIC_MELEE_REACH,
        ray_aabb::{ray_aabb_distance, RayBounds},
    },
    matchmaking::SeatId,
};

use super::runtime::GameplayRuntimeContext;

const PLAYER_HALF_WIDTH: f32 = 0.4;
const PLAYER_EYE_HEIGHT: f32 = 1.62;
const PLAYER_HEIGHT: f32 = 1.8;
const UNIT_EPSILON: f32 = 0.000_1;

#[derive(Clone, Copy)]
pub(super) struct TargetCandidate {
    pub entity: Entity,
    pub seat_id: SeatId,
    pub eye_position: [f32; 3],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CombatTarget {
    pub entity: Entity,
    pub distance: f32,
}

pub(super) fn select_combat_target(
    context: &GameplayRuntimeContext,
    chunks: &Chunks,
    registry: &Registry,
    origin: [f32; 3],
    direction: [f32; 3],
    candidates: &[TargetCandidate],
) -> Result<Option<CombatTarget>, CombatTargetingError> {
    validate_origin(context, origin)?;
    let direction = normalize(direction)?;
    let mut hits = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let bounds = player_bounds(candidate.eye_position)?;
        let distance = ray_aabb_distance(origin, direction, bounds, BASIC_MELEE_REACH)
            .map_err(|_| CombatTargetingError::InvalidTransform)?;
        if let Some(distance) = distance {
            hits.push((distance, candidate.seat_id, candidate.entity));
        }
    }
    hits.sort_by(|left, right| {
        left.0
            .total_cmp(&right.0)
            .then_with(|| left.1.cmp(&right.1))
    });
    let Some((distance, _, entity)) = hits.into_iter().next() else {
        return Ok(None);
    };
    if ray_is_occluded(context, chunks, registry, origin, direction, distance)? {
        return Ok(None);
    }
    Ok(Some(CombatTarget { entity, distance }))
}

fn validate_origin(
    context: &GameplayRuntimeContext,
    origin: [f32; 3],
) -> Result<(), CombatTargetingError> {
    if !origin.into_iter().all(f32::is_finite)
        || !context.playable_bounds.contains_xz(origin[0], origin[2])
        || origin[1] < 0.0
        || origin[1] >= context.max_height as f32
    {
        return Err(CombatTargetingError::InvalidTransform);
    }
    Ok(())
}

fn normalize(direction: [f32; 3]) -> Result<[f32; 3], CombatTargetingError> {
    if !direction.into_iter().all(f32::is_finite) {
        return Err(CombatTargetingError::InvalidTransform);
    }
    let length_squared =
        direction[0] * direction[0] + direction[1] * direction[1] + direction[2] * direction[2];
    if !length_squared.is_finite() || length_squared <= f32::EPSILON {
        return Err(CombatTargetingError::InvalidTransform);
    }
    let length = length_squared.sqrt();
    Ok([
        direction[0] / length,
        direction[1] / length,
        direction[2] / length,
    ])
}

fn player_bounds(eye: [f32; 3]) -> Result<RayBounds, CombatTargetingError> {
    if !eye.into_iter().all(f32::is_finite) {
        return Err(CombatTargetingError::InvalidTransform);
    }
    Ok(RayBounds {
        min: [
            eye[0] - PLAYER_HALF_WIDTH,
            eye[1] - PLAYER_EYE_HEIGHT,
            eye[2] - PLAYER_HALF_WIDTH,
        ],
        max: [
            eye[0] + PLAYER_HALF_WIDTH,
            eye[1] - PLAYER_EYE_HEIGHT + PLAYER_HEIGHT,
            eye[2] + PLAYER_HALF_WIDTH,
        ],
    })
}

fn ray_is_occluded(
    context: &GameplayRuntimeContext,
    chunks: &Chunks,
    registry: &Registry,
    origin: [f32; 3],
    direction: [f32; 3],
    max_distance: f32,
) -> Result<bool, CombatTargetingError> {
    let unavailable = Cell::new(false);
    let get_voxel = |x: i32, y: i32, z: i32| {
        if y < 0 || y >= context.max_height {
            unavailable.set(true);
            return true;
        }
        let coords = ChunkUtils::map_voxel_to_chunk(x, y, z, context.chunk_size);
        if !chunks.is_chunk_ready(&coords) {
            unavailable.set(true);
            return true;
        }
        let voxel_id = chunks.get_voxel(x, y, z);
        if voxel_id == 0 {
            return false;
        }
        if !registry.has_type(voxel_id) {
            unavailable.set(true);
            return true;
        }
        let position = Vec3(x, y, z);
        let aabbs = registry
            .get_block_by_id(voxel_id)
            .get_aabbs(&position, chunks, registry);
        aabbs.len() == 1 && is_unit_cube(&aabbs[0])
    };
    let mut ray_origin = Vec3(origin[0], origin[1], origin[2]);
    let mut ray_direction = Vec3(direction[0], direction[1], direction[2]);
    let mut hit_position = Vec3::default();
    let mut hit_normal = Vec3::default();
    let hit = trace(
        max_distance,
        &get_voxel,
        &mut ray_origin,
        &mut ray_direction,
        &mut hit_position,
        &mut hit_normal,
    );
    if unavailable.get() {
        Err(CombatTargetingError::ChunkUnavailable)
    } else {
        Ok(hit)
    }
}

fn is_unit_cube(aabb: &voxelize::AABB) -> bool {
    [
        aabb.min_x,
        aabb.min_y,
        aabb.min_z,
        1.0 - aabb.max_x,
        1.0 - aabb.max_y,
        1.0 - aabb.max_z,
    ]
    .into_iter()
    .all(|value| value.abs() <= UNIT_EPSILON)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum CombatTargetingError {
    InvalidTransform,
    ChunkUnavailable,
}
