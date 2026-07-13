use voxelize::World;

use super::{
    authority::GameplayAuthority,
    components::{EliminationComp, ExtractionComp, MatchPlayerComp, MiningComp},
    intents::{MiningIntentQueue, QueuedMiningIntent},
    methods::{decode_envelope, send_decode_error, send_error},
    runtime::GameplayRuntimeContext,
};
use crate::contracts::{decode_mining_intent, ErrorCode};

pub(super) fn install_mining_method(world: &mut World) {
    world.set_method_handle("pvp:v1:mining", handle_mining);
}

fn handle_mining(world: &mut World, client_id: &str, payload: &str) {
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
    let intent = match decode_mining_intent(&envelope) {
        Ok(intent) => intent,
        Err(_) => {
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
        .zip(world.read_component::<MiningComp>().get(entity))
        .is_none()
        || world
            .read_component::<EliminationComp>()
            .get(entity)
            .is_none_or(|state| state.record().is_some())
        || world
            .read_component::<ExtractionComp>()
            .get(entity)
            .is_none_or(|state| state.record().is_some())
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

    let queued = QueuedMiningIntent {
        entity,
        client_id: client_id.to_owned(),
        request_id,
        sequence: intent.sequence,
        payload: intent.payload,
    };
    if world
        .write_resource::<MiningIntentQueue>()
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
