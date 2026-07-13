use specs::WriteStorage;
use voxelize::MessageQueues;

use super::{
    combat_targeting::CombatTargetingError,
    components::CombatComp,
    intents::{AttackIntentQueue, QueuedAttackIntent},
    messaging::{queue_error, queue_ok},
    runtime::GameplayRuntimeContext,
};
use crate::{
    contracts::{AttackResolution, AttackResultData, ErrorCode},
    gameplay::combat::CombatError,
};

pub(super) fn queue_attack_result(
    context: &GameplayRuntimeContext,
    queues: &mut MessageQueues,
    combat: &WriteStorage<'_, CombatComp>,
    intent: &QueuedAttackIntent,
    resolution: AttackResolution,
) {
    let Some(state) = combat.get(intent.entity) else {
        return;
    };
    queue_ok(
        queues,
        &context.manifest,
        &intent.client_id,
        intent.request_id,
        AttackResultData {
            accepted_sequence: intent.sequence,
            attack_revision: state.state().revision(),
            resolution,
        },
    );
}

pub(super) fn reject_all_unavailable(
    context: &GameplayRuntimeContext,
    intents: &mut AttackIntentQueue,
    queues: &mut MessageQueues,
) {
    for intent in intents.drain().collect::<Vec<_>>() {
        queue_error(
            queues,
            &context.manifest,
            &intent.client_id,
            intent.request_id,
            ErrorCode::ServiceUnavailable,
            true,
        );
    }
}

pub(super) fn reject_invalid(
    context: &GameplayRuntimeContext,
    queues: &mut MessageQueues,
    client_id: &str,
    request_id: uuid::Uuid,
) {
    queue_error(
        queues,
        &context.manifest,
        client_id,
        request_id,
        ErrorCode::GameInvalidState,
        false,
    );
}

pub(super) fn combat_error(error: CombatError) -> (ErrorCode, bool) {
    match error {
        CombatError::StaleSequence => (ErrorCode::GameStaleSequence, false),
        CombatError::InvalidCooldown
        | CombatError::TimeRegression
        | CombatError::TimeOverflow
        | CombatError::RevisionExhausted => (ErrorCode::ServiceUnavailable, true),
    }
}

pub(super) fn targeting_error(error: CombatTargetingError) -> (ErrorCode, bool) {
    match error {
        CombatTargetingError::InvalidTransform => (ErrorCode::GameInvalidState, false),
        CombatTargetingError::ChunkUnavailable => (ErrorCode::ServiceUnavailable, true),
    }
}
