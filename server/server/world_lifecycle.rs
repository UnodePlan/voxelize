use actix::{fut::ready, ActorFutureExt, Message as ActixMessage, ResponseActFuture, WrapFuture};

use crate::{
    errors::AddWorldError, remove_timing_data_for_world, ClientCancelJoinRequest, StopWorld, World,
    WorldStopSummary,
};

use super::*;

#[derive(ActixMessage)]
#[rtype(result = "Result<(), AddWorldError>")]
pub struct AddWorld {
    pub world: World,
}

impl Handler<AddWorld> for Server {
    type Result = MessageResult<AddWorld>;

    fn handle(&mut self, message: AddWorld, _: &mut Context<Self>) -> Self::Result {
        MessageResult(self.add_world(message.world).map(|_| ()))
    }
}

#[derive(ActixMessage)]
#[rtype(result = "Result<RemoveWorldOutcome, String>")]
pub struct RemoveWorld {
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RemoveWorldOutcome {
    pub removed: bool,
    pub client_ids: Vec<String>,
}

impl Handler<RemoveWorld> for Server {
    type Result = ResponseActFuture<Self, Result<RemoveWorldOutcome, String>>;

    fn handle(&mut self, message: RemoveWorld, _: &mut Context<Self>) -> Self::Result {
        let Some(world) = self.worlds.remove(&message.name) else {
            return Box::pin(ready(Ok(RemoveWorldOutcome {
                removed: false,
                client_ids: Vec::new(),
            })));
        };
        self.removing_worlds.insert(message.name.clone());

        if let Some(generation) = self.world_generations.remove(&message.name) {
            self.pending_world_ticks.remove(&generation);
            self.pending_world_requests.remove(&generation);
        }

        let active_connections: Vec<_> = self
            .connections
            .iter()
            .filter(|(_, (_, world_name, _))| world_name == &message.name)
            .map(|(connection_id, _)| connection_id.clone())
            .collect();
        for connection_id in active_connections {
            if let Some((sender, _, token)) = self.connections.remove(&connection_id) {
                self.lost_sessions
                    .insert(connection_id.clone(), (sender, token));
            }
            self.connection_client_ids.remove(&connection_id);
        }

        let pending_connections: Vec<_> = self
            .pending_joins
            .iter()
            .filter(|(_, pending)| pending.world_name == message.name)
            .map(|(connection_id, _)| connection_id.clone())
            .collect();
        for connection_id in pending_connections {
            if let Some(pending) = self.pending_joins.remove(&connection_id) {
                world.do_send(ClientCancelJoinRequest {
                    id: pending.client_id,
                    join_attempt_id: pending.attempt_id,
                });
                self.lost_sessions
                    .insert(connection_id, (pending.sender, pending.token));
            }
        }
        self.detached_connections
            .retain(|_, detached| detached.world_name != message.name);
        self.pending_detaches
            .retain(|_, detached| detached.world_name != message.name);
        self.pending_rebinds
            .retain(|_, detached| detached.world_name != message.name);

        let world_name = message.name;
        let stop_request = world.send(StopWorld);
        Box::pin(stop_request.into_actor(self).map(move |result, server, _| {
            remove_timing_data_for_world(&world_name);
            server.removing_worlds.remove(&world_name);

            match result {
                Ok(WorldStopSummary { client_ids }) => Ok(RemoveWorldOutcome {
                    removed: true,
                    client_ids,
                }),
                Err(_) => Err("World stopped responding during removal.".to_owned()),
            }
        }))
    }
}
