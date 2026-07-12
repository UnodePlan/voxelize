use crate::ConnectionPrincipal;

use super::*;

/// 由权威 Server Actor 按顺序发出的连接生命周期事件。
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConnectionLifecycleEvent {
    Connected {
        connection_id: String,
        principal: Option<ConnectionPrincipal>,
    },
    JoinCommitted {
        connection_id: String,
        principal: Option<ConnectionPrincipal>,
        world_name: String,
        world_generation: String,
        client_id: String,
        attach_attempt_id: String,
    },
    Disconnected {
        connection_id: String,
        principal: Option<ConnectionPrincipal>,
        world_name: Option<String>,
        world_generation: Option<String>,
        client_id: Option<String>,
        attach_attempt_id: Option<String>,
    },
    Detached {
        connection_id: String,
        principal: ConnectionPrincipal,
        world_name: String,
        world_generation: String,
        client_id: String,
        attach_attempt_id: String,
    },
    Rebound {
        connection_id: String,
        principal: ConnectionPrincipal,
        world_name: String,
        world_generation: String,
        client_id: String,
        attach_attempt_id: String,
    },
    RebindRejected {
        principal: ConnectionPrincipal,
        world_name: String,
        world_generation: String,
        client_id: String,
        attach_attempt_id: String,
    },
}

/// 在 Server Actor 线程内同步调用的非阻塞观察器。
///
/// 实现必须快速返回，且不能同步回调同一个 Server Actor；耗时工作应转交其他任务。
pub trait ConnectionLifecycleObserver: Send + Sync {
    fn observe(&self, event: &ConnectionLifecycleEvent);
}

impl<F> ConnectionLifecycleObserver for F
where
    F: Fn(&ConnectionLifecycleEvent) + Send + Sync,
{
    fn observe(&self, event: &ConnectionLifecycleEvent) {
        self(event);
    }
}

impl Server {
    pub(super) fn observe_connection_lifecycle(&self, event: ConnectionLifecycleEvent) {
        if let Some(observer) = &self.connection_lifecycle_observer {
            observer.observe(&event);
        }
    }

    /// 校验当前 token 后只生成一次断连事件；待重绑记录保留到 World 回调收敛。
    pub(super) fn begin_disconnect_event(
        &mut self,
        connection_id: &str,
        token: &str,
    ) -> Option<ConnectionLifecycleEvent> {
        let principal = self.connection_principals.get(connection_id).cloned();

        if let Some(pending) = self
            .pending_joins
            .get(connection_id)
            .filter(|pending| pending.token == token)
        {
            return Some(ConnectionLifecycleEvent::Disconnected {
                connection_id: connection_id.to_owned(),
                principal,
                world_name: Some(pending.world_name.clone()),
                world_generation: Some(pending.world_generation.clone()),
                client_id: Some(pending.client_id.clone()),
                attach_attempt_id: Some(pending.attempt_id.clone()),
            });
        }

        if let Some((_, world_name, _)) = self
            .connections
            .get(connection_id)
            .filter(|(_, _, current_token)| current_token == token)
        {
            return Some(ConnectionLifecycleEvent::Disconnected {
                connection_id: connection_id.to_owned(),
                principal,
                world_name: Some(world_name.clone()),
                world_generation: self.world_generations.get(world_name).cloned(),
                client_id: Some(
                    self.connection_client_ids
                        .get(connection_id)
                        .cloned()
                        .unwrap_or_else(|| connection_id.to_owned()),
                ),
                attach_attempt_id: self
                    .connection_attach_attempt_ids
                    .get(connection_id)
                    .cloned(),
            });
        }

        let pending_rebind_account = self.pending_rebinds.iter().find_map(|(account, pending)| {
            (pending.connection_id == connection_id
                && pending.connection_token == token
                && !pending.disconnected)
                .then(|| account.clone())
        });
        if let Some(account_id) = pending_rebind_account {
            let (principal, detached, attach_attempt_id) = {
                let pending = self.pending_rebinds.get_mut(&account_id).unwrap();
                pending.disconnected = true;
                (
                    pending.principal.clone(),
                    pending.detached.clone(),
                    pending.connection_token.clone(),
                )
            };
            let generation_is_current = self
                .world_generations
                .get(&detached.world_name)
                .is_some_and(|generation| generation == &detached.world_generation);
            return Some(ConnectionLifecycleEvent::Disconnected {
                connection_id: connection_id.to_owned(),
                principal: Some(principal),
                world_name: generation_is_current.then_some(detached.world_name),
                world_generation: generation_is_current.then_some(detached.world_generation),
                client_id: generation_is_current.then_some(detached.client_id),
                attach_attempt_id: Some(attach_attempt_id),
            });
        }

        if self
            .leaving_sessions
            .get(connection_id)
            .is_some_and(|leaving| leaving.token == token)
            || self
                .lost_sessions
                .get(connection_id)
                .is_some_and(|(_, current_token)| current_token == token)
        {
            return Some(ConnectionLifecycleEvent::Disconnected {
                connection_id: connection_id.to_owned(),
                principal,
                world_name: None,
                world_generation: None,
                client_id: None,
                attach_attempt_id: None,
            });
        }

        None
    }
}
