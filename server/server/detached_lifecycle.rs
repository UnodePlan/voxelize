use actix::{fut::ready, ActorFutureExt, Context, Handler, ResponseActFuture, WrapFuture};
use futures_util::future::join_all;

use crate::{ClientDespawnRequest, ClientDetachOutcome, ClientDetachRequest, ConnectionPrincipal};

use super::{connections::DetachedConnection, *};

impl Server {
    pub(super) fn schedule_client_detach(
        &mut self,
        context: &mut Context<Self>,
        principal: ConnectionPrincipal,
        reservation: DetachedConnection,
        world: Addr<SyncWorld>,
    ) {
        let account_id = principal.account_id.clone();
        self.pending_detaches
            .insert(account_id.clone(), reservation.clone());
        let detach_world = world.clone();
        let detach_request = world.send(ClientDetachRequest {
            id: reservation.client_id.clone(),
        });

        context.spawn(
            detach_request
                .into_actor(self)
                .map(move |result, server, _| {
                    let reservation_matches = server
                        .pending_detaches
                        .get(&account_id)
                        .is_some_and(|current| current == &reservation);
                    if !reservation_matches {
                        return;
                    }
                    server.pending_detaches.remove(&account_id);

                    let world_matches = server
                        .world_generations
                        .get(&reservation.world_name)
                        .is_some_and(|generation| generation == &reservation.world_generation);
                    if matches!(result, Ok(ClientDetachOutcome::Detached)) && world_matches {
                        server
                            .detached_connections
                            .insert(account_id, reservation.clone());
                        server.observe_connection_lifecycle(ConnectionLifecycleEvent::Detached {
                            connection_id: reservation.connection_id,
                            principal,
                            world_name: reservation.world_name,
                            world_generation: reservation.world_generation,
                            client_id: reservation.client_id,
                            attach_attempt_id: reservation.attach_attempt_id,
                        });
                    } else if matches!(result, Ok(ClientDetachOutcome::Detached)) {
                        detach_world.do_send(ClientDespawnRequest {
                            id: reservation.client_id,
                            join_attempt_id: None,
                        });
                    }
                }),
        );
    }
}

/// 移除断连账号的重连登记及 World 中保留的实体。
///
/// generation 为必填项，避免旧超时任务清理复用同名世界的新实例。
#[derive(actix::Message)]
#[rtype(result = "bool")]
pub struct DespawnDetachedPrincipal {
    pub account_id: String,
    pub world_name: String,
    pub world_generation: String,
}

/// 淘汰后按账号移出指定 World；在线连接保留在大厅，掉线实体直接销毁。
#[derive(actix::Message)]
#[rtype(result = "bool")]
pub struct EvictMatchPrincipal {
    pub account_id: String,
    pub world_name: String,
    pub world_generation: String,
}

impl Handler<EvictMatchPrincipal> for Server {
    type Result = ResponseActFuture<Self, bool>;

    fn handle(&mut self, message: EvictMatchPrincipal, _: &mut Context<Self>) -> Self::Result {
        let generation_matches = self
            .world_generations
            .get(&message.world_name)
            .is_some_and(|generation| generation == &message.world_generation);
        if !generation_matches {
            return Box::pin(ready(false));
        }

        let active_connection =
            self.connection_principals
                .iter()
                .find_map(|(connection_id, principal)| {
                    (principal.account_id == message.account_id
                        && self
                            .connections
                            .get(connection_id)
                            .is_some_and(|(_, world_name, _)| world_name == &message.world_name))
                    .then(|| connection_id.clone())
                });
        if let Some(connection_id) = active_connection {
            return Box::pin(self.leave_world(connection_id).map(|_, _, _| true));
        }

        let detached = DespawnDetachedPrincipal {
            account_id: message.account_id,
            world_name: message.world_name,
            world_generation: message.world_generation,
        };
        despawn_detached(self, detached)
    }
}

impl Handler<DespawnDetachedPrincipal> for Server {
    type Result = ResponseActFuture<Self, bool>;

    fn handle(&mut self, message: DespawnDetachedPrincipal, _: &mut Context<Self>) -> Self::Result {
        despawn_detached(self, message)
    }
}

fn despawn_detached(
    server: &mut Server,
    message: DespawnDetachedPrincipal,
) -> ResponseActFuture<Server, bool> {
    let matches_target = |detached: &DetachedConnection| {
        detached.world_name == message.world_name
            && detached.world_generation == message.world_generation
    };
    let mut client_ids = Vec::new();

    if server
        .detached_connections
        .get(&message.account_id)
        .is_some_and(matches_target)
    {
        let detached = server
            .detached_connections
            .remove(&message.account_id)
            .unwrap();
        client_ids.push(detached.client_id);
    }
    if server
        .pending_detaches
        .get(&message.account_id)
        .is_some_and(matches_target)
    {
        let detached = server.pending_detaches.remove(&message.account_id).unwrap();
        client_ids.push(detached.client_id);
    }
    if let Some(pending) = server
        .pending_rebinds
        .get_mut(&message.account_id)
        .filter(|pending| matches_target(&pending.detached) && !pending.despawn_requested)
    {
        // 保留过渡租约供 Disconnect 校验 token；回调看到标记后只做清理，不再提交路由。
        pending.despawn_requested = true;
        client_ids.push(pending.detached.client_id.clone());
    }

    client_ids.sort_unstable();
    client_ids.dedup();
    if client_ids.is_empty() {
        return Box::pin(ready(false));
    }

    let world = server
        .world_generations
        .get(&message.world_name)
        .filter(|generation| *generation == &message.world_generation)
        .and_then(|_| server.worlds.get(&message.world_name))
        .cloned();
    let Some(world) = world else {
        return Box::pin(ready(true));
    };

    Box::pin(
        async move {
            join_all(client_ids.into_iter().map(|id| {
                world.send(ClientDespawnRequest {
                    id,
                    join_attempt_id: None,
                })
            }))
            .await;
            true
        }
        .into_actor(server),
    )
}
