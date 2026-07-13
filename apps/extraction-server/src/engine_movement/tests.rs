use std::time::Duration;

use voxelize::{Block, Chunk, ChunkOptions, PositionComp, Registry, VoxelAccess, WorldConfig};

use super::{
    system::{advance_body, canonical_body, MovementStepContext},
    MovementAxes, MovementIntentComp, MovementUpdate,
};
use crate::match_world::{PlayableBounds, PLAYER_EYE_OFFSET_FROM_CENTER};

const STEP: Duration = Duration::from_millis(50);

fn update(forward: f32, right: f32, jump: bool, direction: [f32; 3]) -> MovementUpdate {
    MovementUpdate {
        movement: MovementAxes {
            forward,
            right,
            jump,
        },
        direction,
        _ignored_position: None,
    }
}

struct PhysicsFixture {
    config: WorldConfig,
    registry: Registry,
    chunk: Chunk,
}

fn physics_fixture(with_wall: bool) -> PhysicsFixture {
    let config = WorldConfig::new().max_height(16).max_light_level(1).build();
    let mut registry = Registry::new();
    registry.register_block(&Block::new("movement-test-solid").id(1).build());
    registry.generate();
    let mut chunk = Chunk::new(
        "movement-test-chunk",
        0,
        0,
        &ChunkOptions {
            size: config.chunk_size,
            max_height: config.max_height,
            sub_chunks: config.sub_chunks,
        },
    );
    for x in 0..config.chunk_size as i32 {
        for z in 0..config.chunk_size as i32 {
            assert!(chunk.set_raw_voxel(x, 0, z, 1));
        }
    }
    if with_wall {
        for y in 1..=4 {
            for z in 0..config.chunk_size as i32 {
                assert!(chunk.set_raw_voxel(3, y, z, 1));
            }
        }
    }
    PhysicsFixture {
        config,
        registry,
        chunk,
    }
}

fn player_at(center: [f32; 3]) -> (voxelize::RigidBody, PositionComp) {
    let mut body = canonical_body();
    body.set_position(center[0], center[1], center[2]);
    let position = PositionComp::new(
        center[0],
        center[1] + PLAYER_EYE_OFFSET_FROM_CENTER,
        center[2],
    );
    (body, position)
}

fn step_body(
    body: &mut voxelize::RigidBody,
    position: &mut PositionComp,
    intent: &mut MovementIntentComp,
    active: bool,
    now: Duration,
    space_ready: bool,
    fixture: &PhysicsFixture,
) {
    advance_body(
        body,
        position,
        intent,
        MovementStepContext {
            active,
            now: Some(now),
            delta: STEP.as_secs_f32(),
            space_ready,
            bounds: PlayableBounds::EXTRACTION,
            space: &fixture.chunk,
            registry: &fixture.registry,
            config: &fixture.config,
        },
    );
}

#[test]
fn wire_schema_accepts_intent_and_ignores_position() {
    let parsed = serde_json::from_str::<MovementUpdate>(
        r#"{
            "movement":{"forward":1.0,"right":0.0,"jump":false},
            "direction":[0.0,0.0,-1.0],
            "position":[149.0,63.0,149.0]
        }"#,
    )
    .unwrap();
    let mut intent = MovementIntentComp::default();
    assert_eq!(
        intent.accept(parsed, Duration::ZERO),
        Some([0.0, 0.0, -1.0])
    );
    assert_eq!(
        intent.take_control(Duration::ZERO),
        Some(([0.0, -1.0], false))
    );

    assert!(serde_json::from_str::<MovementUpdate>(
        r#"{"movement":{"forward":0.0,"right":0.0,"jump":false},"direction":[0,0,-1],"flying":true}"#,
    )
    .is_err());
}

#[test]
fn axes_direction_and_input_rate_are_bounded() {
    let mut intent = MovementIntentComp::default();
    assert!(intent
        .accept(update(0.8, 0.8, false, [0.0, 0.0, -1.0]), Duration::ZERO)
        .is_none());
    assert!(intent
        .accept(update(0.0, 0.0, false, [0.0, 1.0, 0.0]), Duration::ZERO)
        .is_none());

    for _ in 0..6 {
        assert!(intent
            .accept(update(0.0, 0.0, false, [0.0, 0.0, -1.0]), Duration::ZERO)
            .is_some());
    }
    assert!(intent
        .accept(update(0.0, 0.0, false, [0.0, 0.0, -1.0]), Duration::ZERO)
        .is_none());
    assert!(intent
        .accept(
            update(0.0, 0.0, false, [0.0, 0.0, -1.0]),
            Duration::from_millis(100),
        )
        .is_some());
    assert_eq!(
        intent.take_control(Duration::from_millis(351)),
        None,
        "超过 250ms 的输入不能继续驱动玩家"
    );
}

#[test]
fn gravity_lands_and_syncs_the_eye_position() {
    let fixture = physics_fixture(false);
    let (mut body, mut position) = player_at([1.5, 5.0, 1.5]);
    let mut intent = MovementIntentComp::default();
    for tick in 1..=40 {
        step_body(
            &mut body,
            &mut position,
            &mut intent,
            true,
            STEP * tick,
            true,
            &fixture,
        );
    }

    assert_eq!(body.at_rest_y(), -1);
    assert!((body.aabb.min_y - 1.0).abs() < 0.001);
    assert!(
        (position.0 .1 - (body.get_position().1 + PLAYER_EYE_OFFSET_FROM_CENTER)).abs() < 0.001
    );
}

#[test]
fn repeated_legal_steps_cannot_cross_a_voxel_wall() {
    let fixture = physics_fixture(true);
    let (mut body, mut position) = player_at([1.5, 1.9, 1.5]);
    body.resting.1 = -1;
    let mut intent = MovementIntentComp::default();
    for tick in 1..=30 {
        let now = STEP * tick;
        assert!(intent
            .accept(update(1.0, 0.0, false, [1.0, 0.0, 0.0]), now)
            .is_some());
        step_body(
            &mut body,
            &mut position,
            &mut intent,
            true,
            now,
            true,
            &fixture,
        );
    }

    assert!(body.aabb.max_x <= 3.000_1);
    assert!(body.get_position().0 < 3.0);
}

#[test]
fn held_jump_and_air_jump_do_not_retrigger() {
    let fixture = physics_fixture(false);
    let (mut body, mut position) = player_at([1.5, 1.9, 1.5]);
    body.resting.1 = -1;
    let mut intent = MovementIntentComp::default();
    assert!(intent
        .accept(update(0.0, 0.0, true, [0.0, 0.0, -1.0]), STEP)
        .is_some());
    step_body(
        &mut body,
        &mut position,
        &mut intent,
        true,
        STEP,
        true,
        &fixture,
    );
    assert!(body.velocity.1 > 0.0);

    for tick in 2..=60 {
        let now = STEP * tick;
        assert!(intent
            .accept(update(0.0, 0.0, true, [0.0, 0.0, -1.0]), now)
            .is_some());
        step_body(
            &mut body,
            &mut position,
            &mut intent,
            true,
            now,
            true,
            &fixture,
        );
    }
    assert_eq!(body.at_rest_y(), -1);
    assert!(body.velocity.1.abs() < 0.001);

    let release_at = STEP * 61;
    assert!(intent
        .accept(update(0.0, 0.0, false, [0.0, 0.0, -1.0]), release_at)
        .is_some());
    let jump_again_at = STEP * 62;
    assert!(intent
        .accept(update(0.0, 0.0, true, [0.0, 0.0, -1.0]), jump_again_at,)
        .is_some());
    step_body(
        &mut body,
        &mut position,
        &mut intent,
        true,
        jump_again_at,
        true,
        &fixture,
    );
    assert!(body.velocity.1 > 0.0);

    let (mut airborne, mut airborne_position) = player_at([5.5, 5.0, 5.5]);
    let (mut reference, mut reference_position) = player_at([5.5, 5.0, 5.5]);
    let mut air_intent = MovementIntentComp::default();
    let mut reference_intent = MovementIntentComp::default();
    assert!(air_intent
        .accept(update(0.0, 0.0, true, [0.0, 0.0, -1.0]), STEP)
        .is_some());
    step_body(
        &mut airborne,
        &mut airborne_position,
        &mut air_intent,
        true,
        STEP,
        true,
        &fixture,
    );
    step_body(
        &mut reference,
        &mut reference_position,
        &mut reference_intent,
        true,
        STEP,
        true,
        &fixture,
    );
    assert!((airborne.velocity.1 - reference.velocity.1).abs() < f32::EPSILON);
}

#[test]
fn unavailable_space_and_flying_state_fail_closed() {
    let fixture = physics_fixture(false);
    let (mut body, mut position) = player_at([1.5, 5.0, 1.5]);
    body.gravity_multiplier = 0.0;
    body.velocity.set(2.0, 4.0, 2.0);
    let mut intent = MovementIntentComp::default();
    step_body(
        &mut body,
        &mut position,
        &mut intent,
        true,
        STEP,
        false,
        &fixture,
    );
    assert_eq!(body.gravity_multiplier, 1.0);
    assert_eq!(body.velocity.to_arr(), [0.0, 0.0, 0.0]);

    step_body(
        &mut body,
        &mut position,
        &mut intent,
        true,
        STEP * 2,
        true,
        &fixture,
    );
    assert!(body.velocity.1 < 0.0);

    let (mut denied_body, mut denied_position) = player_at([4.5, 5.0, 4.5]);
    let mut denied_intent = MovementIntentComp::default();
    assert!(denied_intent
        .accept(update(1.0, 0.0, true, [1.0, 0.0, 0.0]), STEP)
        .is_some());
    step_body(
        &mut denied_body,
        &mut denied_position,
        &mut denied_intent,
        false,
        STEP,
        true,
        &fixture,
    );
    assert!((denied_body.get_position().0 - 4.5).abs() < 0.001);
    assert!(denied_body.velocity.1 < 0.0, "拒绝控制后重力仍由服务端推进");
}

#[test]
fn ghost_shape_is_rebuilt_and_world_boundary_rolls_back() {
    let fixture = physics_fixture(false);
    let (mut body, mut position) = player_at([1.5, 5.0, 1.5]);
    body.aabb.max_x = body.aabb.min_x;
    let mut intent = MovementIntentComp::default();
    step_body(
        &mut body,
        &mut position,
        &mut intent,
        true,
        STEP,
        true,
        &fixture,
    );
    assert!((body.aabb.width() - 0.8).abs() < 0.001);
    assert!((body.aabb.height() - 1.8).abs() < 0.001);

    let (mut boundary_body, mut boundary_position) = player_at([149.5, 5.0, 1.5]);
    let mut boundary_intent = MovementIntentComp::default();
    assert!(boundary_intent
        .accept(update(1.0, 0.0, false, [1.0, 0.0, 0.0]), STEP)
        .is_some());
    step_body(
        &mut boundary_body,
        &mut boundary_position,
        &mut boundary_intent,
        true,
        STEP,
        true,
        &fixture,
    );
    assert!((boundary_body.get_position().0 - 149.5).abs() < 0.001);
    assert_eq!(boundary_body.velocity.to_arr(), [0.0, 0.0, 0.0]);

    // 眼睛仍在世界高度内时，完整碰撞体越过上边界也必须回滚。
    let (mut ceiling_body, mut ceiling_position) = player_at([5.5, 15.0, 5.5]);
    ceiling_body.velocity.1 = 3.5;
    let mut ceiling_intent = MovementIntentComp::default();
    step_body(
        &mut ceiling_body,
        &mut ceiling_position,
        &mut ceiling_intent,
        true,
        STEP,
        true,
        &fixture,
    );
    assert!((ceiling_body.get_position().1 - 15.0).abs() < 0.001);
    assert_eq!(ceiling_body.velocity.to_arr(), [0.0, 0.0, 0.0]);
}
