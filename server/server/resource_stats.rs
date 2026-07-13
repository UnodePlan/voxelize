use actix::{Handler, MessageResult};

use super::Server;

/// 只读的 Server actor 资源计数，用于验证生命周期对象最终被释放。
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ServerResourceSnapshot {
    pub worlds: usize,
    pub world_generations: usize,
    pub removing_worlds: usize,
    pub lost_sessions: usize,
    pub transport_sessions: usize,
    pub connections: usize,
    pub connection_principals: usize,
    pub pending_joins: usize,
    pub leaving_sessions: usize,
    pub connection_client_ids: usize,
    pub connection_attach_attempt_ids: usize,
    pub detached_connections: usize,
    pub pending_detaches: usize,
    pub pending_rebinds: usize,
    pub pending_world_request_routes: usize,
    pub pending_world_requests: usize,
    pub pending_world_ticks: usize,
}

#[derive(actix::Message)]
#[rtype(result = "ServerResourceSnapshot")]
pub struct GetServerResourceSnapshot;

impl ServerResourceSnapshot {
    fn from_server(server: &Server) -> Self {
        Self {
            worlds: server.worlds.len(),
            world_generations: server.world_generations.len(),
            removing_worlds: server.removing_worlds.len(),
            lost_sessions: server.lost_sessions.len(),
            transport_sessions: server.transport_sessions.len(),
            connections: server.connections.len(),
            connection_principals: server.connection_principals.len(),
            pending_joins: server.pending_joins.len(),
            leaving_sessions: server.leaving_sessions.len(),
            connection_client_ids: server.connection_client_ids.len(),
            connection_attach_attempt_ids: server.connection_attach_attempt_ids.len(),
            detached_connections: server.detached_connections.len(),
            pending_detaches: server.pending_detaches.len(),
            pending_rebinds: server.pending_rebinds.len(),
            pending_world_request_routes: server.pending_world_requests.len(),
            pending_world_requests: server.pending_world_requests.values().sum(),
            pending_world_ticks: server.pending_world_ticks.len(),
        }
    }
}

impl Handler<GetServerResourceSnapshot> for Server {
    type Result = MessageResult<GetServerResourceSnapshot>;

    fn handle(
        &mut self,
        _: GetServerResourceSnapshot,
        _: &mut actix::Context<Self>,
    ) -> Self::Result {
        MessageResult(ServerResourceSnapshot::from_server(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConnectionPrincipal;

    #[test]
    fn snapshot_counts_every_internal_resource_without_exposing_identity() {
        let mut server = Server::new().debug(false).build();
        let (sender, _) = super::super::WsSender::channel(1);

        server
            .world_generations
            .insert("arena".to_owned(), "generation".to_owned());
        server.removing_worlds.insert("old-arena".to_owned());
        server
            .lost_sessions
            .insert("lost".to_owned(), (sender.clone(), "lost-token".to_owned()));
        server
            .transport_sessions
            .insert("transport".to_owned(), sender.clone());
        server.connections.insert(
            "active".to_owned(),
            (
                sender.clone(),
                "arena".to_owned(),
                "active-token".to_owned(),
            ),
        );
        server.connection_principals.insert(
            "active".to_owned(),
            ConnectionPrincipal::new("account", "session"),
        );
        server
            .connection_client_ids
            .insert("active".to_owned(), "client".to_owned());
        server
            .connection_attach_attempt_ids
            .insert("active".to_owned(), "attempt".to_owned());
        server.pending_joins.insert(
            "pending".to_owned(),
            super::super::PendingJoin {
                sender: sender.clone(),
                world_name: "arena".to_owned(),
                token: "pending-token".to_owned(),
                client_id: "pending-client".to_owned(),
                attempt_id: "pending-attempt".to_owned(),
                world_generation: "generation".to_owned(),
            },
        );
        server.leaving_sessions.insert(
            "leaving".to_owned(),
            super::super::LeavingSession {
                sender: sender.clone(),
                token: "leaving-token".to_owned(),
            },
        );
        let detached = super::super::DetachedConnection {
            connection_id: "detached".to_owned(),
            world_name: "arena".to_owned(),
            client_id: "detached-client".to_owned(),
            attach_attempt_id: "detached-attempt".to_owned(),
            world_generation: "generation".to_owned(),
            connection_token: "detached-token".to_owned(),
        };
        server
            .detached_connections
            .insert("detached-account".to_owned(), detached.clone());
        server
            .pending_detaches
            .insert("detached".to_owned(), detached.clone());
        server.pending_rebinds.insert(
            "rebind-account".to_owned(),
            super::super::PendingRebind {
                detached,
                connection_id: "rebind".to_owned(),
                connection_token: "rebind-token".to_owned(),
                sender,
                principal: ConnectionPrincipal::new("rebind-account", "rebind-session"),
                disconnected: false,
                despawn_requested: false,
            },
        );
        server.pending_world_requests.insert("arena".to_owned(), 2);
        server
            .pending_world_requests
            .insert("other-arena".to_owned(), 3);
        server.pending_world_ticks.insert("arena".to_owned());

        let snapshot = ServerResourceSnapshot::from_server(&server);
        assert_eq!(snapshot.worlds, 0);
        assert_eq!(snapshot.world_generations, 1);
        assert_eq!(snapshot.removing_worlds, 1);
        assert_eq!(snapshot.lost_sessions, 1);
        assert_eq!(snapshot.transport_sessions, 1);
        assert_eq!(snapshot.connections, 1);
        assert_eq!(snapshot.connection_principals, 1);
        assert_eq!(snapshot.pending_joins, 1);
        assert_eq!(snapshot.leaving_sessions, 1);
        assert_eq!(snapshot.connection_client_ids, 1);
        assert_eq!(snapshot.connection_attach_attempt_ids, 1);
        assert_eq!(snapshot.detached_connections, 1);
        assert_eq!(snapshot.pending_detaches, 1);
        assert_eq!(snapshot.pending_rebinds, 1);
        assert_eq!(snapshot.pending_world_request_routes, 2);
        assert_eq!(snapshot.pending_world_requests, 5);
        assert_eq!(snapshot.pending_world_ticks, 1);
    }
}
