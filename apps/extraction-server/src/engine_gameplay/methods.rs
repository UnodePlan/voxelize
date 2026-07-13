use serde::Deserialize;
use serde_json::Value;
use voxelize::{MessageQueues, World};

use super::mining_method::install_mining_method;
use super::{
    authority::GameplayAuthority,
    components::{FixedEquipmentComp, MatchPlayerComp, MiningComp, ResourceInventoryComp},
    intents::{DropSlotIntentQueue, QueuedDropSlotIntent},
    messaging::{queue_error, queue_ok, PlayerGameplayState},
    runtime::GameplayRuntimeContext,
};
use crate::contracts::{decode_drop_slot_intent, decode_protocol_envelope, ErrorCode};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EmptyPayload {}

pub(super) fn install_gameplay_methods(world: &mut World) {
    world.set_method_handle("pvp:v1:drop-slot", handle_drop_slot);
    world.set_method_handle("pvp:v1:get-state", handle_get_state);
    install_mining_method(world);
}

fn handle_drop_slot(world: &mut World, client_id: &str, payload: &str) {
    let context = {
        let context = world.read_resource::<GameplayRuntimeContext>();
        (*context).clone()
    };
    let envelope = match decode_envelope(payload, &context) {
        Ok(envelope) => envelope,
        Err(error) => {
            send_decode_error(world, &context, client_id, error);
            return;
        }
    };
    let request_id = envelope.request_id();
    let Ok(intent) = decode_drop_slot_intent(&envelope) else {
        send_error(
            world,
            &context,
            client_id,
            request_id,
            ErrorCode::RequestMalformed,
            false,
        );
        return;
    };
    let authority = {
        let authority = world.read_resource::<GameplayAuthority>();
        (*authority).clone()
    };
    if authority.authorize_client(world, client_id).is_none() {
        send_error(
            world,
            &context,
            client_id,
            request_id,
            ErrorCode::GameInvalidState,
            false,
        );
        return;
    }
    let Some(entity) = world.clients().get(client_id).map(|client| client.entity) else {
        return;
    };
    if world
        .read_component::<MatchPlayerComp>()
        .get(entity)
        .is_none()
    {
        send_error(
            world,
            &context,
            client_id,
            request_id,
            ErrorCode::GameInvalidState,
            false,
        );
        return;
    }

    let queued = QueuedDropSlotIntent {
        entity,
        client_id: client_id.to_owned(),
        request_id,
        sequence: intent.sequence,
        payload: intent.payload,
    };
    let full = world
        .write_resource::<DropSlotIntentQueue>()
        .push(queued)
        .is_err();
    if full {
        send_error(
            world,
            &context,
            client_id,
            request_id,
            ErrorCode::ServiceUnavailable,
            true,
        );
    }
}

fn handle_get_state(world: &mut World, client_id: &str, payload: &str) {
    let context = {
        let context = world.read_resource::<GameplayRuntimeContext>();
        (*context).clone()
    };
    let envelope = match decode_envelope(payload, &context) {
        Ok(envelope) => envelope,
        Err(error) => {
            send_decode_error(world, &context, client_id, error);
            return;
        }
    };
    let request_id = envelope.request_id();
    if envelope.decode_intent::<EmptyPayload>().is_err() {
        send_error(
            world,
            &context,
            client_id,
            request_id,
            ErrorCode::RequestMalformed,
            false,
        );
        return;
    }
    let authority = {
        let authority = world.read_resource::<GameplayAuthority>();
        (*authority).clone()
    };
    if authority.authorize_client(world, client_id).is_none() {
        send_error(
            world,
            &context,
            client_id,
            request_id,
            ErrorCode::GameInvalidState,
            false,
        );
        return;
    }
    let Some(entity) = world.clients().get(client_id).map(|client| client.entity) else {
        return;
    };
    let inventories = world.read_component::<ResourceInventoryComp>();
    let equipment = world.read_component::<FixedEquipmentComp>();
    let mining = world.read_component::<MiningComp>();
    let state = inventories
        .get(entity)
        .zip(equipment.get(entity))
        .zip(mining.get(entity))
        .and_then(|((inventory, equipment), mining)| {
            PlayerGameplayState::new(&context, inventory, equipment, mining)
        });
    drop(inventories);
    drop(equipment);
    drop(mining);
    match state {
        Some(state) => queue_ok(
            &mut world.write_resource::<MessageQueues>(),
            &context.manifest,
            client_id,
            request_id,
            state,
        ),
        None => send_error(
            world,
            &context,
            client_id,
            request_id,
            ErrorCode::GameInvalidState,
            false,
        ),
    }
}

pub(super) fn decode_envelope(
    payload: &str,
    context: &GameplayRuntimeContext,
) -> Result<crate::contracts::ProtocolEnvelope, EnvelopeDecodeError> {
    let value = serde_json::from_str::<Value>(payload).map_err(|_| EnvelopeDecodeError {
        request_id: None,
        code: ErrorCode::RequestMalformed,
    })?;
    let request_id = value
        .get("requestId")
        .and_then(Value::as_str)
        .and_then(|value| uuid::Uuid::parse_str(value).ok());
    let code = match value.get("protocolVersion").and_then(Value::as_u64) {
        Some(version) if version != u64::from(context.manifest.protocol_version) => {
            ErrorCode::ProtocolUnsupportedVersion
        }
        _ => ErrorCode::RequestMalformed,
    };
    decode_protocol_envelope(value, &context.manifest)
        .map_err(|_| EnvelopeDecodeError { request_id, code })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct EnvelopeDecodeError {
    request_id: Option<uuid::Uuid>,
    code: ErrorCode,
}

pub(super) fn send_decode_error(
    world: &mut World,
    context: &GameplayRuntimeContext,
    client_id: &str,
    error: EnvelopeDecodeError,
) {
    if let Some(request_id) = error.request_id {
        send_error(world, context, client_id, request_id, error.code, false);
    }
}

pub(super) fn send_error(
    world: &mut World,
    context: &GameplayRuntimeContext,
    client_id: &str,
    request_id: uuid::Uuid,
    code: ErrorCode,
    retryable: bool,
) {
    queue_error(
        &mut world.write_resource::<MessageQueues>(),
        &context.manifest,
        client_id,
        request_id,
        code,
        retryable,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{contracts::bundled_manifest, gameplay::config::GAMEPLAY_V1};

    fn context() -> GameplayRuntimeContext {
        GameplayRuntimeContext::new(
            uuid::Uuid::from_u128(1),
            GAMEPLAY_V1,
            bundled_manifest().unwrap(),
            crate::match_world::PlayableBounds::EXTRACTION,
            1,
            64,
            16,
        )
    }

    #[test]
    fn envelope_errors_preserve_request_identity_and_error_taxonomy() {
        let context = context();
        let request_id = "11111111-1111-4111-8111-111111111111";
        let unsupported = decode_envelope(
            &format!(
                r#"{{"protocolVersion":2,"type":"intent","requestId":"{request_id}","sequence":1,"payload":{{}}}}"#
            ),
            &context,
        )
        .unwrap_err();
        assert_eq!(unsupported.code, ErrorCode::ProtocolUnsupportedVersion);
        assert_eq!(unsupported.request_id.unwrap().to_string(), request_id);

        let malformed = decode_envelope(
            &format!(
                r#"{{"protocolVersion":1,"type":"intent","requestId":"{request_id}","sequence":1,"payload":{{}},"damage":20}}"#
            ),
            &context,
        )
        .unwrap_err();
        assert_eq!(malformed.code, ErrorCode::RequestMalformed);
        assert_eq!(malformed.request_id.unwrap().to_string(), request_id);

        let invalid_json = decode_envelope("{", &context).unwrap_err();
        assert_eq!(invalid_json.request_id, None);
        assert_eq!(invalid_json.code, ErrorCode::RequestMalformed);
    }
}
