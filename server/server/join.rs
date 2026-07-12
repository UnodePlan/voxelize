use actix::{fut::ready, ActorFutureExt, ResponseActFuture, WrapFuture};
use nanoid::nanoid;

use crate::{
    ClientDespawnRequest, ClientForgetJoinAttemptRequest, ClientJoinError, ConnectionSecurityMode,
};

use super::*;

impl Server {
    pub(super) fn begin_join(
        &mut self,
        connection_id: String,
        data: Message,
    ) -> ResponseActFuture<Self, Option<String>> {
        let request: OnJoinRequest = match serde_json::from_str(&data.json) {
            Ok(request) => request,
            Err(_) => return Box::pin(ready(Some("Invalid join request.".to_owned()))),
        };
        let Some(world) = self.worlds.get(&request.world).cloned() else {
            return Box::pin(ready(Some("World does not exist.".to_owned())));
        };
        let Some(world_generation) = self.world_generations.get(&request.world).cloned() else {
            return Box::pin(ready(Some("World is unavailable.".to_owned())));
        };
        if self
            .pending_world_requests
            .get(&world_generation)
            .copied()
            .unwrap_or_default()
            >= self.http_config.world_request_capacity_value()
        {
            return Box::pin(ready(Some("World is busy, please reconnect.".to_owned())));
        }
        let Some((sender, token)) = self.lost_sessions.remove(&connection_id) else {
            return Box::pin(ready(Some(
                "Connection is already joining or joined.".to_owned(),
            )));
        };

        let principal = self.connection_principals.get(&connection_id).cloned();
        let client_id = if principal.is_some() {
            nanoid!()
        } else {
            connection_id.clone()
        };
        let attempt_id = nanoid!();
        let world_name = request.world;
        let username = if self.http_config.security_mode() == ConnectionSecurityMode::PublicStrict {
            "Player".to_owned()
        } else {
            request.username
        };
        self.pending_joins.insert(
            connection_id.clone(),
            PendingJoin {
                sender: sender.clone(),
                world_name: world_name.clone(),
                token: token.clone(),
                client_id: client_id.clone(),
                attempt_id: attempt_id.clone(),
                world_generation: world_generation.clone(),
            },
        );
        *self
            .pending_world_requests
            .entry(world_generation.clone())
            .or_default() += 1;

        let join = ClientJoinRequest {
            id: client_id,
            username,
            sender,
            preferences: request
                .flat_preferences
                .merge(request.preferences.unwrap_or_default()),
            principal,
            join_attempt_id: attempt_id.clone(),
        };

        let join_request = world.send(join);
        Box::pin(join_request.into_actor(self).map(move |result, server, _| {
            server.release_world_request(&world_generation);
            if result.is_ok() {
                world.do_send(ClientForgetJoinAttemptRequest {
                    join_attempt_id: attempt_id.clone(),
                });
            }
            let pending_matches = server
                .pending_joins
                .get(&connection_id)
                .is_some_and(|pending| {
                    pending.token == token
                        && pending.attempt_id == attempt_id
                        && pending.world_generation == world_generation
                });
            let world_matches = server
                .world_generations
                .get(&world_name)
                .is_some_and(|generation| generation == &world_generation);

            match result {
                Ok(Ok(receipt)) if pending_matches && world_matches => {
                    let pending = server.pending_joins.remove(&connection_id).unwrap();
                    server.connections.insert(
                        connection_id.clone(),
                        (pending.sender, pending.world_name, pending.token),
                    );
                    server
                        .connection_client_ids
                        .insert(connection_id, receipt.client_id);
                    None
                }
                Ok(Ok(receipt)) => {
                    world.do_send(ClientDespawnRequest {
                        id: receipt.client_id,
                        join_attempt_id: Some(receipt.join_attempt_id),
                    });
                    server.restore_pending_join(
                        &connection_id,
                        &token,
                        &attempt_id,
                        &world_generation,
                    );
                    Some("Join was cancelled.".to_owned())
                }
                Ok(Err(error)) => {
                    server.restore_pending_join(
                        &connection_id,
                        &token,
                        &attempt_id,
                        &world_generation,
                    );
                    Some(join_error_message(error))
                }
                Err(_) => {
                    server.restore_pending_join(
                        &connection_id,
                        &token,
                        &attempt_id,
                        &world_generation,
                    );
                    Some("World is unavailable.".to_owned())
                }
            }
        }))
    }

    fn restore_pending_join(
        &mut self,
        connection_id: &str,
        token: &str,
        attempt_id: &str,
        world_generation: &str,
    ) {
        let matches = self
            .pending_joins
            .get(connection_id)
            .is_some_and(|pending| {
                pending.token == token
                    && pending.attempt_id == attempt_id
                    && pending.world_generation == world_generation
            });
        if !matches {
            return;
        }
        let Some(pending) = self.pending_joins.remove(connection_id) else {
            return;
        };
        if self.connection_principals.contains_key(connection_id)
            || self.http_config.security_mode() == ConnectionSecurityMode::Legacy
        {
            self.lost_sessions
                .insert(connection_id.to_owned(), (pending.sender, pending.token));
        }
    }
}

fn join_error_message(error: ClientJoinError) -> String {
    match error {
        ClientJoinError::WorldNotReady(_) => "World is not accepting clients.".to_owned(),
        ClientJoinError::WorldFull { .. } => "World is full.".to_owned(),
        ClientJoinError::DuplicateClient | ClientJoinError::DuplicatePrincipal => {
            "Client is already in this world.".to_owned()
        }
        ClientJoinError::JoinCancelled => "Join was cancelled.".to_owned(),
    }
}
