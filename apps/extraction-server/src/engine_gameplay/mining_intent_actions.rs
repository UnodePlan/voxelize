use serde::Serialize;

use super::{
    intents::QueuedMiningIntent,
    messaging::{queue_error, queue_ok},
    mining_intents::MiningIntentAccess,
    mining_validation::{validate_mining_target, MiningValidationAccess, MiningValidationError},
};
use crate::{
    contracts::{ErrorCode, MiningIdleReason},
    gameplay::mining::{MiningStateError, MiningTarget, VoxelCoordinate},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MiningIntentResult {
    mining_revision: u32,
}

pub(super) fn apply_start(
    access: &mut MiningIntentAccess<'_, '_>,
    intent: QueuedMiningIntent,
    voxel: VoxelCoordinate,
) {
    let validation = validate_for_entity(access, intent.entity, voxel);
    let Ok(target) = validation else {
        let (reason, code, retryable) = validation.unwrap_err().protocol();
        reject_intent(
            access,
            intent.entity,
            &intent.client_id,
            intent.request_id,
            intent.sequence,
            reason,
            (code, retryable),
        );
        return;
    };
    let now = access.now;
    apply_state_operation(access, &intent, |state| {
        state.start(intent.sequence, target, now)
    });
}

pub(super) fn apply_maintain(access: &mut MiningIntentAccess<'_, '_>, intent: QueuedMiningIntent) {
    let active = access
        .mining
        .get(intent.entity)
        .and_then(|component| component.state().active_target());
    if let Some(expected) = active {
        match validate_for_entity(access, intent.entity, expected.voxel) {
            Ok(actual) if actual == expected => {}
            Ok(_) => {
                reject_intent(
                    access,
                    intent.entity,
                    &intent.client_id,
                    intent.request_id,
                    intent.sequence,
                    MiningIdleReason::InvalidBlock,
                    (ErrorCode::GameInvalidState, false),
                );
                return;
            }
            Err(error) => {
                let (reason, code, retryable) = error.protocol();
                reject_intent(
                    access,
                    intent.entity,
                    &intent.client_id,
                    intent.request_id,
                    intent.sequence,
                    reason,
                    (code, retryable),
                );
                return;
            }
        }
    }
    let now = access.now;
    apply_state_operation(access, &intent, |state| {
        state.maintain(intent.sequence, now)
    });
}

pub(super) fn apply_cancel(access: &mut MiningIntentAccess<'_, '_>, intent: QueuedMiningIntent) {
    apply_state_operation(access, &intent, |state| state.cancel(intent.sequence));
}

fn validate_for_entity(
    access: &MiningIntentAccess<'_, '_>,
    entity: specs::Entity,
    voxel: VoxelCoordinate,
) -> Result<MiningTarget, MiningValidationError> {
    validate_mining_target(
        voxel,
        MiningValidationAccess {
            context: access.context,
            chunks: access.chunks,
            harvested: access.harvested,
            equipment: access
                .equipment
                .get(entity)
                .ok_or(MiningValidationError::InvalidState)?,
            position: access
                .positions
                .get(entity)
                .ok_or(MiningValidationError::InvalidState)?,
            direction: access
                .directions
                .get(entity)
                .ok_or(MiningValidationError::InvalidState)?,
        },
    )
}

fn apply_state_operation<F>(
    access: &mut MiningIntentAccess<'_, '_>,
    intent: &QueuedMiningIntent,
    operation: F,
) where
    F: FnOnce(&mut crate::gameplay::mining::MiningState) -> Result<(), MiningStateError>,
{
    let Some(component) = access.mining.get_mut(intent.entity) else {
        queue_intent_error(
            access,
            &intent.client_id,
            intent.request_id,
            ErrorCode::GameInvalidState,
            false,
        );
        return;
    };
    let before = component.state().revision();
    let result = operation(component.state_mut());
    let revision = component.state().revision();
    if revision != before {
        access.dirty.mark(&intent.client_id, intent.entity, false);
    }
    match result {
        Ok(()) => queue_ok(
            access.queues,
            &access.context.manifest,
            &intent.client_id,
            intent.request_id,
            MiningIntentResult {
                mining_revision: revision,
            },
        ),
        Err(error) => {
            let (code, retryable) = map_state_error(error);
            queue_intent_error(
                access,
                &intent.client_id,
                intent.request_id,
                code,
                retryable,
            );
        }
    }
}

pub(super) fn reject_intent(
    access: &mut MiningIntentAccess<'_, '_>,
    entity: specs::Entity,
    client_id: &str,
    request_id: uuid::Uuid,
    sequence: u32,
    reason: MiningIdleReason,
    protocol_error: (ErrorCode, bool),
) {
    let Some(component) = access.mining.get_mut(entity) else {
        queue_intent_error(
            access,
            client_id,
            request_id,
            ErrorCode::GameInvalidState,
            false,
        );
        return;
    };
    let before = component.state().revision();
    match component.state_mut().reject(sequence, reason) {
        Ok(()) => {
            if component.state().revision() != before {
                access.dirty.mark(client_id, entity, false);
            }
            queue_intent_error(
                access,
                client_id,
                request_id,
                protocol_error.0,
                protocol_error.1,
            );
        }
        Err(error) => {
            let (code, retryable) = map_state_error(error);
            queue_intent_error(access, client_id, request_id, code, retryable);
        }
    }
}

pub(super) fn queue_intent_error(
    access: &mut MiningIntentAccess<'_, '_>,
    client_id: &str,
    request_id: uuid::Uuid,
    code: ErrorCode,
    retryable: bool,
) {
    queue_error(
        access.queues,
        &access.context.manifest,
        client_id,
        request_id,
        code,
        retryable,
    );
}

fn map_state_error(error: MiningStateError) -> (ErrorCode, bool) {
    match error {
        MiningStateError::StaleSequence => (ErrorCode::GameStaleSequence, false),
        MiningStateError::NoActiveAttempt => (ErrorCode::GameInvalidState, false),
        MiningStateError::TimeRegression
        | MiningStateError::RevisionExhausted
        | MiningStateError::InvalidDuration
        | MiningStateError::DurationOverflow
        | MiningStateError::InvariantViolation => (ErrorCode::ServiceUnavailable, true),
    }
}
