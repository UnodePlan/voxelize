use actix::{fut::ready, ActorFutureExt, ResponseActFuture, WrapFuture};
use futures_util::future::Either;

use crate::{ClientCancelJoinRequest, ClientDespawnRequest, ConnectionSecurityMode};

use super::*;

struct WorldForward {
    world_name: String,
    generation: String,
    client_id: String,
    data: Message,
}

enum LeaveCleanup {
    CancelJoin(ClientCancelJoinRequest),
    Despawn(ClientDespawnRequest),
}

impl Server {
    fn prepare_non_join(
        &mut self,
        connection_id: &str,
        message_type: MessageType,
        data: Message,
    ) -> Result<Option<WorldForward>, String> {
        if message_type == MessageType::Action {
            if self.http_config.security_mode() == ConnectionSecurityMode::PublicStrict {
                return Err("Action messages are disabled.".to_owned());
            }
            self.on_action(&data)?;
            return Ok(None);
        }

        if message_type == MessageType::Transport
            || self.transport_sessions.contains_key(connection_id)
        {
            return self.prepare_transport(connection_id, data).map(Some);
        }

        let route = self.connections.get(connection_id).map(|(_, world, _)| {
            (
                world.clone(),
                self.connection_client_ids
                    .get(connection_id)
                    .cloned()
                    .unwrap_or_else(|| connection_id.to_owned()),
            )
        });
        let Some((world_name, client_id)) = route else {
            if self.pending_joins.contains_key(connection_id) {
                return Err("Join is still pending.".to_owned());
            }
            return Err("You are not connected to a world!".to_owned());
        };
        let Some(generation) = self.world_generations.get(&world_name).cloned() else {
            return Err("World is unavailable.".to_owned());
        };
        Ok(Some(WorldForward {
            world_name,
            generation,
            client_id,
            data,
        }))
    }

    pub(super) fn leave_world(
        &mut self,
        connection_id: String,
    ) -> ResponseActFuture<Self, Option<String>> {
        let leaving = if let Some(pending) = self.pending_joins.remove(&connection_id) {
            let world = self
                .world_generations
                .get(&pending.world_name)
                .filter(|generation| *generation == &pending.world_generation)
                .and_then(|_| self.worlds.get(&pending.world_name))
                .cloned();
            Some((
                pending.sender,
                pending.token,
                world,
                LeaveCleanup::CancelJoin(ClientCancelJoinRequest {
                    id: pending.client_id,
                    join_attempt_id: pending.attempt_id,
                }),
            ))
        } else if let Some((sender, world_name, token)) = self.connections.remove(&connection_id) {
            let client_id = self
                .connection_client_ids
                .remove(&connection_id)
                .unwrap_or_else(|| connection_id.clone());
            self.connection_attach_attempt_ids.remove(&connection_id);
            Some((
                sender,
                token,
                self.worlds.get(&world_name).cloned(),
                LeaveCleanup::Despawn(ClientDespawnRequest {
                    id: client_id,
                    join_attempt_id: None,
                }),
            ))
        } else {
            None
        };
        let Some((sender, token, world, request)) = leaving else {
            return Box::pin(ready(None));
        };

        self.leaving_sessions.insert(
            connection_id.clone(),
            LeavingSession {
                sender,
                token: token.clone(),
            },
        );
        let Some(world) = world else {
            self.finish_leave(&connection_id, &token);
            return Box::pin(ready(None));
        };

        let cleanup = match request {
            LeaveCleanup::CancelJoin(request) => Either::Left(world.send(request)),
            LeaveCleanup::Despawn(request) => Either::Right(world.send(request)),
        };
        Box::pin(cleanup.into_actor(self).map(move |_, server, _| {
            server.finish_leave(&connection_id, &token);
            None
        }))
    }

    pub(super) fn finish_leave(&mut self, connection_id: &str, token: &str) {
        let matches = self
            .leaving_sessions
            .get(connection_id)
            .is_some_and(|current| current.token == token);
        if !matches {
            return;
        }
        let leaving = self.leaving_sessions.remove(connection_id).unwrap();

        let session_is_unclaimed = !self.lost_sessions.contains_key(connection_id)
            && !self.connections.contains_key(connection_id)
            && !self.pending_joins.contains_key(connection_id);
        let can_rejoin = self.connection_principals.contains_key(connection_id)
            || self.http_config.security_mode() == ConnectionSecurityMode::Legacy;
        if session_is_unclaimed && can_rejoin {
            self.lost_sessions
                .insert(connection_id.to_owned(), (leaving.sender, leaving.token));
        }
    }

    fn prepare_transport(
        &self,
        connection_id: &str,
        data: Message,
    ) -> Result<WorldForward, String> {
        if !self.transport_sessions.contains_key(connection_id) {
            return Err("Transport access denied.".to_owned());
        }
        if data.text.is_empty() {
            return Err("Transport message is missing a world name.".to_owned());
        }
        let world_name = data.text.clone();
        let Some(generation) = self.world_generations.get(&world_name).cloned() else {
            return Err("Transport target world does not exist.".to_owned());
        };
        Ok(WorldForward {
            world_name,
            generation,
            client_id: connection_id.to_owned(),
            data,
        })
    }

    fn queue_world_request(
        &mut self,
        forward: WorldForward,
    ) -> ResponseActFuture<Self, Option<String>> {
        let Some(world) = self.worlds.get(&forward.world_name).cloned() else {
            return Box::pin(ready(Some("World is unavailable.".to_owned())));
        };
        let pending = self
            .pending_world_requests
            .entry(forward.generation.clone())
            .or_default();
        if *pending >= self.http_config.world_request_capacity_value() {
            return Box::pin(ready(Some("World is busy, please reconnect.".to_owned())));
        }
        *pending += 1;
        let generation = forward.generation;

        Box::pin(
            async move {
                world
                    .send(ClientRequest {
                        client_id: forward.client_id,
                        data: forward.data,
                    })
                    .await
            }
            .into_actor(self)
            .map(move |result, server, _| {
                server.release_world_request(&generation);
                result.err().map(|_| "World is unavailable.".to_owned())
            }),
        )
    }

    pub(super) fn release_world_request(&mut self, generation: &str) {
        if let Some(pending) = self.pending_world_requests.get_mut(generation) {
            *pending = pending.saturating_sub(1);
            if *pending == 0 {
                self.pending_world_requests.remove(generation);
            }
        }
    }

    fn on_action(&mut self, data: &Message) -> Result<(), String> {
        let request: OnActionRequest =
            serde_json::from_str(&data.json).map_err(|_| "Invalid action request.".to_owned())?;
        let action = request.action.to_lowercase();
        let Some(handle) = self.action_handles.get(&action).cloned() else {
            return Err("Unknown action.".to_owned());
        };
        handle(request.data, self);
        Ok(())
    }
}

impl Handler<ClientMessage> for Server {
    type Result = ResponseActFuture<Self, Option<String>>;

    fn handle(&mut self, message: ClientMessage, _: &mut Context<Self>) -> Self::Result {
        let message_type = match MessageType::try_from(message.data.r#type) {
            Ok(message_type) => message_type,
            Err(_) => return Box::pin(ready(Some("Invalid message type.".to_owned()))),
        };
        if message_type == MessageType::Join {
            return self.begin_join(message.id, message.data);
        }
        if message_type == MessageType::Leave {
            return self.leave_world(message.id);
        }

        match self.prepare_non_join(&message.id, message_type, message.data) {
            Ok(Some(forward)) => self.queue_world_request(forward),
            Ok(None) => Box::pin(ready(None)),
            Err(error) => Box::pin(ready(Some(error))),
        }
    }
}
