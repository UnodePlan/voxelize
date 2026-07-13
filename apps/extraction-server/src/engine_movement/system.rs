use std::time::Duration;

use specs::{Entities, Join, ReadExpect, ReadStorage, System, WriteStorage};
use voxelize::{
    ChunkUtils, Chunks, Clients, IDComp, Physics, PositionComp, Registry, RigidBody, RigidBodyComp,
    Stats, Vec2, VoxelAccess, WorldConfig, AABB,
};

use crate::{
    engine_gameplay::{EliminationComp, ExtractionComp, GameplayAuthority, MatchPlayerComp},
    match_world::{
        PlayableBounds, PLAYER_BODY_DEPTH, PLAYER_BODY_HEIGHT, PLAYER_BODY_WIDTH,
        PLAYER_EYE_OFFSET_FROM_CENTER,
    },
};

use super::MovementIntentComp;

const MAX_HORIZONTAL_SPEED: f32 = 6.0;
const JUMP_IMPULSE: f32 = 8.0;
const BODY_SIZE_EPSILON: f32 = 0.000_1;
const MAX_PHYSICS_DELTA: f32 = 0.05;

pub(super) struct AuthoritativeMovementSystem;

impl<'a> System<'a> for AuthoritativeMovementSystem {
    type SystemData = (
        Entities<'a>,
        ReadExpect<'a, Stats>,
        ReadExpect<'a, Registry>,
        ReadExpect<'a, WorldConfig>,
        ReadExpect<'a, Chunks>,
        ReadExpect<'a, Clients>,
        ReadExpect<'a, GameplayAuthority>,
        ReadExpect<'a, PlayableBounds>,
        ReadStorage<'a, IDComp>,
        ReadStorage<'a, MatchPlayerComp>,
        ReadStorage<'a, EliminationComp>,
        ReadStorage<'a, ExtractionComp>,
        WriteStorage<'a, MovementIntentComp>,
        WriteStorage<'a, RigidBodyComp>,
        WriteStorage<'a, PositionComp>,
    );

    fn run(&mut self, data: Self::SystemData) {
        let (
            entities,
            stats,
            registry,
            config,
            chunks,
            clients,
            authority,
            bounds,
            ids,
            players,
            eliminations,
            extractions,
            mut intents,
            mut bodies,
            mut positions,
        ) = data;
        if stats.preloading {
            return;
        }
        let now = authority.monotonic_now();

        (
            &entities,
            &ids,
            &players,
            &eliminations,
            &extractions,
            &mut intents,
            &mut bodies,
            &mut positions,
        )
            .join()
            .for_each(
                |(entity, id, player, elimination, extraction, intent, body, position)| {
                    let active = !elimination.is_eliminated()
                        && !extraction.is_settlement_pending()
                        && authority.allows_entity(&clients, entity, &id.0, player.account_id());
                    let space_ready = chunks_ready(&chunks, &body.0, config.chunk_size);
                    advance_body(
                        &mut body.0,
                        position,
                        intent,
                        MovementStepContext {
                            active,
                            now,
                            delta: stats.delta,
                            space_ready,
                            bounds: *bounds,
                            space: &*chunks,
                            registry: &registry,
                            config: &config,
                        },
                    );
                },
            );
    }
}

pub(super) struct MovementStepContext<'a> {
    pub active: bool,
    pub now: Option<Duration>,
    pub delta: f32,
    pub space_ready: bool,
    pub bounds: PlayableBounds,
    pub space: &'a dyn VoxelAccess,
    pub registry: &'a Registry,
    pub config: &'a WorldConfig,
}

pub(super) fn advance_body(
    body: &mut RigidBody,
    position: &mut PositionComp,
    intent: &mut MovementIntentComp,
    step: MovementStepContext<'_>,
) {
    let MovementStepContext {
        active,
        now,
        delta,
        space_ready,
        bounds,
        space,
        registry,
        config,
    } = step;
    // 每 tick 恢复服务端固定刚体属性，旧 flying/ghost 状态不能跨重连保留。
    body.gravity_multiplier = 1.0;
    body.auto_step = false;
    body.mass = 1.0;
    body.restitution = 0.0;

    if !ensure_valid_body(body, position, bounds, config.max_height) {
        intent.stop();
        stop_all_motion(body);
        return;
    }
    if !delta.is_finite() || delta <= 0.0 || delta > MAX_PHYSICS_DELTA || !space_ready {
        intent.stop();
        stop_all_motion(body);
        sync_eye_position(body, position);
        return;
    }

    body.forces.set(0.0, 0.0, 0.0);
    body.impulses.set(0.0, 0.0, 0.0);
    let control = if active {
        now.and_then(|now| intent.take_control(now))
    } else {
        intent.stop();
        None
    };
    let (horizontal, jump) = control.unwrap_or(([0.0, 0.0], false));
    body.velocity.0 = horizontal[0] * MAX_HORIZONTAL_SPEED;
    body.velocity.2 = horizontal[1] * MAX_HORIZONTAL_SPEED;
    if jump && body.at_rest_y() < 0 {
        body.apply_impulse(0.0, JUMP_IMPULSE, 0.0);
    }

    let previous = body.clone();
    // 复用 Voxelize 的 swept-AABB；边界失败只回滚本 tick，不自行实现碰撞几何。
    Physics::iterate_body(body, delta, space, registry, config);
    if !body_transform_is_valid(body, bounds, config.max_height) {
        *body = previous;
        stop_all_motion(body);
    }
    sync_eye_position(body, position);
}

fn chunks_ready(chunks: &Chunks, body: &RigidBody, chunk_size: usize) -> bool {
    let center = body.get_position();
    if ![center.0, center.1, center.2]
        .into_iter()
        .all(f32::is_finite)
    {
        return false;
    }
    let coords = ChunkUtils::map_voxel_to_chunk(
        center.0.floor() as i32,
        center.1.floor() as i32,
        center.2.floor() as i32,
        chunk_size,
    );
    // Chunks 会把缺失数据读成 Air，因此 sweep 前必须显式验证当前区块及邻区。
    if !chunks.is_chunk_ready(&coords) {
        return false;
    }
    for dx in -1..=1 {
        for dz in -1..=1 {
            let neighbor = Vec2(coords.0 + dx, coords.1 + dz);
            if chunks.is_within_world(&neighbor) && !chunks.is_chunk_ready(&neighbor) {
                return false;
            }
        }
    }
    true
}

fn ensure_valid_body(
    body: &mut RigidBody,
    position: &PositionComp,
    bounds: PlayableBounds,
    max_height: usize,
) -> bool {
    let dimensions = [body.aabb.width(), body.aabb.height(), body.aabb.depth()];
    let canonical = dimensions.into_iter().all(f32::is_finite)
        && (dimensions[0] - PLAYER_BODY_WIDTH).abs() <= BODY_SIZE_EPSILON
        && (dimensions[1] - PLAYER_BODY_HEIGHT).abs() <= BODY_SIZE_EPSILON
        && (dimensions[2] - PLAYER_BODY_DEPTH).abs() <= BODY_SIZE_EPSILON;
    if canonical && body_transform_is_valid(body, bounds, max_height) {
        return true;
    }
    let eye = position.0.to_arr();
    if !eye.into_iter().all(f32::is_finite) {
        return false;
    }
    let mut replacement = canonical_body();
    replacement.set_position(eye[0], eye[1] - PLAYER_EYE_OFFSET_FROM_CENTER, eye[2]);
    if !body_transform_is_valid(&replacement, bounds, max_height) {
        return false;
    }
    *body = replacement;
    true
}

pub(super) fn canonical_body() -> RigidBody {
    RigidBody::new(
        &AABB::new()
            .scale_x(PLAYER_BODY_WIDTH)
            .scale_y(PLAYER_BODY_HEIGHT)
            .scale_z(PLAYER_BODY_DEPTH)
            .build(),
    )
    .build()
}

fn body_transform_is_valid(body: &RigidBody, bounds: PlayableBounds, max_height: usize) -> bool {
    let eye_y = body.get_position().1 + PLAYER_EYE_OFFSET_FROM_CENTER;
    [
        body.aabb.min_x,
        body.aabb.min_y,
        body.aabb.min_z,
        body.aabb.max_x,
        body.aabb.max_y,
        body.aabb.max_z,
        body.velocity.0,
        body.velocity.1,
        body.velocity.2,
        eye_y,
    ]
    .into_iter()
    .all(f32::is_finite)
        && body.aabb.min_x >= bounds.min_inclusive
        && body.aabb.max_x < bounds.max_exclusive
        && body.aabb.min_y >= 0.0
        && body.aabb.max_y < max_height as f32
        && body.aabb.min_z >= bounds.min_inclusive
        && body.aabb.max_z < bounds.max_exclusive
        && eye_y >= 0.0
        && eye_y < max_height as f32
}

fn sync_eye_position(body: &RigidBody, position: &mut PositionComp) {
    let center = body.get_position();
    position
        .0
        .set(center.0, center.1 + PLAYER_EYE_OFFSET_FROM_CENTER, center.2);
}

fn stop_all_motion(body: &mut RigidBody) {
    body.velocity.set(0.0, 0.0, 0.0);
    body.forces.set(0.0, 0.0, 0.0);
    body.impulses.set(0.0, 0.0, 0.0);
}
