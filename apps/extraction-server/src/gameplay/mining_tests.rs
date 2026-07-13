use std::time::Duration;

use super::{
    config::GAMEPLAY_V1,
    mining::{MiningState, MiningStateError, MiningTarget, MiningTick, VoxelCoordinate},
};
use crate::contracts::{MiningIdleReason, MiningStateData, ResourceKey};

fn target(resource: ResourceKey, x: i32) -> MiningTarget {
    MiningTarget {
        voxel: VoxelCoordinate::new(x, 20, 30),
        voxel_id: x as u32,
        resource,
    }
}

#[test]
fn each_resource_becomes_ready_at_its_exact_duration_boundary() {
    let cases = [
        (ResourceKey::Dirt, 500_u64),
        (ResourceKey::Gold, 1_500),
        (ResourceKey::Diamond, 3_000),
    ];

    for (index, (resource, required_ms)) in cases.into_iter().enumerate() {
        let required = GAMEPLAY_V1.mining_duration(resource);
        assert_eq!(required, Duration::from_millis(required_ms));

        let expected_target = target(resource, index as i32 + 1);
        let mut state = MiningState::default();
        state
            .start(1, expected_target, Duration::ZERO)
            .expect("start should succeed");

        let just_before = required - Duration::from_millis(1);
        state
            .maintain(2, just_before)
            .expect("maintain should keep the attempt alive");
        assert!(
            matches!(
                state
                    .sample(
                        just_before,
                        required,
                        GAMEPLAY_V1.mining_maintain_grace,
                        GAMEPLAY_V1.mining_sync_interval,
                    )
                    .expect("sampling before the boundary should succeed"),
                MiningTick::Progressed | MiningTick::Unchanged
            ),
            "{resource:?} must remain active one millisecond early"
        );
        assert_eq!(
            state
                .sample(
                    required,
                    required,
                    GAMEPLAY_V1.mining_maintain_grace,
                    GAMEPLAY_V1.mining_sync_interval,
                )
                .expect("sampling at the boundary should succeed"),
            MiningTick::Ready(expected_target),
            "{resource:?} should complete at its exact duration",
        );
    }
}

#[test]
fn maintain_grace_is_inclusive_and_resets_only_after_the_boundary() {
    let mut state = MiningState::default();
    let expected_target = target(ResourceKey::Diamond, 4);
    let required = Duration::from_secs(10);
    let grace = GAMEPLAY_V1.mining_maintain_grace;

    state
        .start(1, expected_target, Duration::ZERO)
        .expect("start should succeed");
    assert!(matches!(
        state
            .sample(
                grace - Duration::from_millis(1),
                required,
                grace,
                GAMEPLAY_V1.mining_sync_interval,
            )
            .expect("sampling before grace should succeed"),
        MiningTick::Progressed | MiningTick::Unchanged
    ));
    assert_eq!(state.active_target(), Some(expected_target));

    assert!(matches!(
        state
            .sample(grace, required, grace, GAMEPLAY_V1.mining_sync_interval,)
            .expect("sampling at grace should succeed"),
        MiningTick::Progressed | MiningTick::Unchanged
    ));
    assert_eq!(state.active_target(), Some(expected_target));

    assert_eq!(
        state
            .sample(
                grace + Duration::from_millis(1),
                required,
                grace,
                GAMEPLAY_V1.mining_sync_interval,
            )
            .expect("sampling after grace should succeed"),
        MiningTick::Reset,
    );
    assert_eq!(state.active_target(), None);
    assert_eq!(
        state.snapshot(None),
        Ok(MiningStateData::Idle {
            accepted_sequence: Some(1),
            reason: MiningIdleReason::TimedOut,
        })
    );
}

#[test]
fn sub_millisecond_sync_interval_is_rejected_without_mutation() {
    let expected_target = target(ResourceKey::Dirt, 7);
    let mut state = MiningState::default();
    state
        .start(1, expected_target, Duration::ZERO)
        .expect("start should succeed");

    assert_eq!(
        state.sample(
            Duration::from_millis(1),
            GAMEPLAY_V1.mining_duration(ResourceKey::Dirt),
            GAMEPLAY_V1.mining_maintain_grace,
            Duration::from_micros(500),
        ),
        Err(MiningStateError::InvalidDuration),
    );
    assert_eq!(state.revision(), 1);
    assert_eq!(state.active_target(), Some(expected_target));
}

#[test]
fn completion_is_prepared_on_a_clone_before_asset_commit() {
    let expected_target = target(ResourceKey::Gold, 8);
    let mut state = MiningState::default();
    state
        .start(1, expected_target, Duration::ZERO)
        .expect("start should succeed");

    let completed = state
        .completed()
        .expect("completion preflight should succeed");
    assert_eq!(state.active_target(), Some(expected_target));
    assert_eq!(state.revision(), 1);
    assert_eq!(
        completed.snapshot(None),
        Ok(MiningStateData::Idle {
            accepted_sequence: Some(1),
            reason: MiningIdleReason::Completed,
        })
    );
    assert_eq!(completed.revision(), 2);
}

#[test]
fn sequence_orders_start_maintain_target_switch_and_cancel() {
    let first = target(ResourceKey::Dirt, 5);
    let second = target(ResourceKey::Gold, 6);
    let mut state = MiningState::default();

    state
        .start(10, first, Duration::ZERO)
        .expect("first start should succeed");
    assert_eq!(state.active_target(), Some(first));
    assert_eq!(state.revision(), 1);

    assert_eq!(
        state.start(10, second, Duration::from_millis(50)),
        Err(MiningStateError::StaleSequence),
    );
    assert_eq!(state.active_target(), Some(first));
    assert_eq!(state.revision(), 1);

    state
        .maintain(11, Duration::from_millis(100))
        .expect("newer maintain should succeed");
    assert_eq!(state.active_target(), Some(first));
    assert_eq!(state.revision(), 2);

    let switched_at = Duration::from_millis(150);
    state
        .start(12, second, switched_at)
        .expect("newer start should switch targets");
    assert_eq!(state.active_target(), Some(second));
    assert_eq!(
        state.ready_at(GAMEPLAY_V1.mining_duration(ResourceKey::Gold)),
        Some(switched_at + GAMEPLAY_V1.mining_duration(ResourceKey::Gold)),
    );
    assert_eq!(state.revision(), 3);

    state.cancel(13).expect("newer cancel should succeed");
    assert_eq!(
        state.snapshot(None),
        Ok(MiningStateData::Idle {
            accepted_sequence: Some(13),
            reason: MiningIdleReason::Cancelled,
        })
    );
    assert_eq!(state.revision(), 4);

    assert_eq!(
        state.maintain(14, Duration::from_millis(200)),
        Err(MiningStateError::NoActiveAttempt),
    );
    assert_eq!(
        state.snapshot(None),
        Ok(MiningStateData::Idle {
            accepted_sequence: Some(14),
            reason: MiningIdleReason::InvalidBlock,
        })
    );
    assert_eq!(state.revision(), 5);
    assert_eq!(state.cancel(14), Err(MiningStateError::StaleSequence));
    assert_eq!(state.revision(), 5);
}
