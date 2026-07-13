use voxelize::World;

use super::{
    authority::GameplayAuthority,
    components::{EliminationComp, MatchPlayerComp},
    intents::{AttackIntentQueue, QueuedAttackIntent},
    methods::{decode_envelope, send_decode_error, send_error},
    runtime::GameplayRuntimeContext,
};
use crate::contracts::{decode_attack_intent, ErrorCode};

pub(super) fn install_attack_method(world: &mut World) {
    world.set_method_handle("pvp:v1:attack", handle_attack);
}

fn handle_attack(world: &mut World, client_id: &str, payload: &str) {
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
    let Ok(intent) = decode_attack_intent(&envelope) else {
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
    let Some(entity) = world.clients().get(client_id).map(|client| client.entity) else {
        return;
    };
    let active_player = authority.authorize_client(world, client_id).is_some()
        && world
            .read_component::<MatchPlayerComp>()
            .get(entity)
            .is_some()
        && world
            .read_component::<EliminationComp>()
            .get(entity)
            .is_some_and(|state| state.record().is_none());
    if !active_player {
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

    let queued = QueuedAttackIntent {
        entity,
        client_id: client_id.to_owned(),
        request_id,
        sequence: intent.sequence,
        payload: intent.payload,
    };
    if world
        .write_resource::<AttackIntentQueue>()
        .push(queued)
        .is_err()
    {
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
