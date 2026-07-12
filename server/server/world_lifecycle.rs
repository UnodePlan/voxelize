use std::fmt;

use actix::{fut::ready, ActorFutureExt, Message as ActixMessage, ResponseActFuture, WrapFuture};

use crate::{
    errors::AddWorldError, remove_timing_data_for_world, ClientCancelJoinRequest, GetInfo, Prepare,
    StopWorld, World, WorldLifecycleState, WorldStopSummary,
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
#[rtype(result = "Result<PrepareWorldOutcome, PrepareWorldError>")]
pub struct PrepareWorld {
    pub name: String,
    /// 可选的比较后准备保护；首次查询 generation 时传 None。
    pub expected_generation: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PrepareWorldOutcome {
    pub generation: String,
    pub lifecycle: WorldLifecycleState,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PrepareWorldError {
    NotFound,
    GenerationMismatch {
        expected: String,
        actual: Option<String>,
    },
    Unavailable,
}

impl fmt::Display for PrepareWorldError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("world was not found"),
            Self::GenerationMismatch { expected, actual } => {
                write!(
                    formatter,
                    "world generation mismatch: expected {expected}, actual {actual:?}"
                )
            }
            Self::Unavailable => formatter.write_str("world is unavailable"),
        }
    }
}

impl std::error::Error for PrepareWorldError {}

impl Handler<PrepareWorld> for Server {
    type Result = ResponseActFuture<Self, Result<PrepareWorldOutcome, PrepareWorldError>>;

    fn handle(&mut self, message: PrepareWorld, _: &mut Context<Self>) -> Self::Result {
        let Some(generation) = self.world_generations.get(&message.name).cloned() else {
            return Box::pin(ready(Err(PrepareWorldError::NotFound)));
        };
        if let Some(expected) = message.expected_generation {
            if expected != generation {
                return Box::pin(ready(Err(PrepareWorldError::GenerationMismatch {
                    expected,
                    actual: Some(generation),
                })));
            }
        }
        let Some(world) = self.worlds.get(&message.name).cloned() else {
            return Box::pin(ready(Err(PrepareWorldError::NotFound)));
        };

        let world_name = message.name;
        let expected_callback_generation = generation.clone();
        Box::pin(
            async move {
                world
                    .send(Prepare)
                    .await
                    .map_err(|_| PrepareWorldError::Unavailable)?;
                world
                    .send(GetInfo)
                    .await
                    .map_err(|_| PrepareWorldError::Unavailable)
            }
            .into_actor(self)
            .map(move |result, server, _| {
                let actual = server.world_generations.get(&world_name).cloned();
                if actual.as_ref() != Some(&expected_callback_generation) {
                    return Err(PrepareWorldError::GenerationMismatch {
                        expected: expected_callback_generation,
                        actual,
                    });
                }
                let info = result?;
                Ok(PrepareWorldOutcome {
                    generation,
                    lifecycle: info.lifecycle,
                })
            }),
        )
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
            self.connection_attach_attempt_ids.remove(&connection_id);
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
        // 正在重绑的记录仍持有活跃 socket；移除 generation 已足以阻止回调提交，
        // 记录必须保留到回调自行收敛，才能让会话撤销和 Disconnect 命中过渡态。

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
