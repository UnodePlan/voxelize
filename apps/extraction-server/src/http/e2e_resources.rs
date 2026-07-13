use actix::Addr;
use actix_web::{
    http::header::{CacheControl, CacheDirective},
    web, HttpRequest, HttpResponse,
};
use serde::Serialize;
use voxelize::{
    world_lifecycle_resource_snapshot, GetAllWorldStats, GetServerResourceSnapshot, Server,
    ServerResourceSnapshot, WorldStatsResponse,
};

use super::{error::ApiError, session::required_session, AppState};
use crate::matchmaking::CoordinatorResourceSnapshot;

pub(super) fn configure(config: &mut web::ServiceConfig) {
    config.route("/api/e2e/resources", web::get().to(resources));
}

async fn resources(
    request: HttpRequest,
    state: web::Data<AppState>,
    server: web::Data<Addr<Server>>,
) -> Result<HttpResponse, ApiError> {
    required_session(&request, &state).await?;
    let matchmaking = state
        .matchmaking()
        .ok_or_else(ApiError::service_unavailable)?;
    let server_snapshot = server
        .send(GetServerResourceSnapshot)
        .await
        .map_err(|_| ApiError::service_unavailable())?;
    let worlds = server
        .send(GetAllWorldStats)
        .await
        .map_err(|_| ApiError::service_unavailable())?;
    if worlds.len() != server_snapshot.worlds {
        return Err(ApiError::service_unavailable());
    }
    let coordinator = matchmaking.resource_snapshot().await?;

    Ok(HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoStore]))
        .json(E2eResourceResponse::new(
            server_snapshot,
            coordinator,
            worlds,
        )))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct E2eResourceResponse {
    server: ServerCounts,
    coordinator: CoordinatorCounts,
    memory: MemoryCounts,
    worlds: Vec<WorldCounts>,
}

impl E2eResourceResponse {
    fn new(
        server: ServerResourceSnapshot,
        coordinator: CoordinatorResourceSnapshot,
        worlds: Vec<WorldStatsResponse>,
    ) -> Self {
        Self {
            server: server.into(),
            coordinator: coordinator.into(),
            memory: MemoryCounts::current(),
            worlds: worlds.into_iter().map(WorldCounts::from).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MemoryCounts {
    live_allocated_bytes: u64,
    peak_allocated_bytes: u64,
    live_allocations: u64,
    allocation_count: u64,
    deallocation_count: u64,
    reallocation_count: u64,
    live_world_instances: usize,
    world_background_tasks: usize,
}

impl MemoryCounts {
    fn current() -> Self {
        let allocator = crate::e2e_allocator::snapshot();
        let lifecycle = world_lifecycle_resource_snapshot();
        Self {
            live_allocated_bytes: allocator.live_bytes,
            peak_allocated_bytes: allocator.peak_bytes,
            live_allocations: allocator.live_allocations,
            allocation_count: allocator.allocation_count,
            deallocation_count: allocator.deallocation_count,
            reallocation_count: allocator.reallocation_count,
            live_world_instances: lifecycle.live_world_instances,
            world_background_tasks: lifecycle.world_background_tasks,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServerCounts {
    worlds: usize,
    world_generations: usize,
    removing_worlds: usize,
    lost_sessions: usize,
    transport_sessions: usize,
    connections: usize,
    connection_principals: usize,
    pending_joins: usize,
    leaving_sessions: usize,
    connection_client_ids: usize,
    connection_attach_attempt_ids: usize,
    detached_connections: usize,
    pending_detaches: usize,
    pending_rebinds: usize,
    pending_world_request_routes: usize,
    pending_world_requests: usize,
    pending_world_ticks: usize,
}

impl From<ServerResourceSnapshot> for ServerCounts {
    fn from(snapshot: ServerResourceSnapshot) -> Self {
        Self {
            worlds: snapshot.worlds,
            world_generations: snapshot.world_generations,
            removing_worlds: snapshot.removing_worlds,
            lost_sessions: snapshot.lost_sessions,
            transport_sessions: snapshot.transport_sessions,
            connections: snapshot.connections,
            connection_principals: snapshot.connection_principals,
            pending_joins: snapshot.pending_joins,
            leaving_sessions: snapshot.leaving_sessions,
            connection_client_ids: snapshot.connection_client_ids,
            connection_attach_attempt_ids: snapshot.connection_attach_attempt_ids,
            detached_connections: snapshot.detached_connections,
            pending_detaches: snapshot.pending_detaches,
            pending_rebinds: snapshot.pending_rebinds,
            pending_world_request_routes: snapshot.pending_world_request_routes,
            pending_world_requests: snapshot.pending_world_requests,
            pending_world_ticks: snapshot.pending_world_ticks,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CoordinatorCounts {
    queued_accounts: usize,
    connected_accounts: usize,
    connection_routes: usize,
    live_matches: usize,
    pending_settlements: usize,
    pending_despawns: usize,
    hard_deadline_tasks: usize,
    ticker_pending: bool,
    runtime_generations: usize,
    runtime_owned_matches: usize,
    runtime_forced_eliminations: usize,
    runtime_hard_deadlines: usize,
}

impl From<CoordinatorResourceSnapshot> for CoordinatorCounts {
    fn from(snapshot: CoordinatorResourceSnapshot) -> Self {
        Self {
            queued_accounts: snapshot.queued_accounts,
            connected_accounts: snapshot.connected_accounts,
            connection_routes: snapshot.connection_routes,
            live_matches: snapshot.live_matches,
            pending_settlements: snapshot.pending_settlements,
            pending_despawns: snapshot.pending_despawns,
            hard_deadline_tasks: snapshot.hard_deadline_tasks,
            ticker_pending: snapshot.ticker_pending,
            runtime_generations: snapshot.runtime_generations,
            runtime_owned_matches: snapshot.runtime_owned_matches,
            runtime_forced_eliminations: snapshot.runtime_forced_eliminations,
            runtime_hard_deadlines: snapshot.runtime_hard_deadlines,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WorldCounts {
    client_count: usize,
    entity_count: usize,
    message_queue_critical: usize,
    message_queue_normal: usize,
    message_queue_bulk: usize,
    encoded_pending: usize,
    encoded_processed: usize,
}

impl From<WorldStatsResponse> for WorldCounts {
    fn from(snapshot: WorldStatsResponse) -> Self {
        Self {
            client_count: snapshot.client_count,
            entity_count: snapshot.entity_count,
            message_queue_critical: snapshot.message_queue_critical,
            message_queue_normal: snapshot.message_queue_normal,
            message_queue_bulk: snapshot.message_queue_bulk,
            encoded_pending: snapshot.encoded_pending,
            encoded_processed: snapshot.encoded_processed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_is_count_only_and_omits_world_identity() {
        let response = E2eResourceResponse::new(
            ServerResourceSnapshot {
                worlds: 1,
                ..ServerResourceSnapshot::default()
            },
            CoordinatorResourceSnapshot {
                queued_accounts: 0,
                connected_accounts: 10,
                connection_routes: 10,
                live_matches: 1,
                pending_settlements: 0,
                pending_despawns: 0,
                hard_deadline_tasks: 1,
                ticker_pending: false,
                runtime_generations: 1,
                runtime_owned_matches: 1,
                runtime_forced_eliminations: 1,
                runtime_hard_deadlines: 1,
            },
            vec![WorldStatsResponse {
                name: "sensitive-world-id".to_owned(),
                client_count: 10,
                entity_count: 11,
                message_queue_critical: 1,
                message_queue_normal: 2,
                message_queue_bulk: 3,
                encoded_pending: 4,
                encoded_processed: 5,
            }],
        );
        let json = serde_json::to_value(response).unwrap();

        assert_eq!(json["server"]["worlds"], 1);
        assert_eq!(json["coordinator"]["hardDeadlineTasks"], 1);
        assert_eq!(json["coordinator"]["runtimeOwnedMatches"], 1);
        // 这些计数覆盖整个测试进程，并行 World 测试可能在此刻持有实例或任务。
        for key in [
            "liveAllocatedBytes",
            "peakAllocatedBytes",
            "liveAllocations",
            "allocationCount",
            "deallocationCount",
            "reallocationCount",
            "liveWorldInstances",
            "worldBackgroundTasks",
        ] {
            assert!(json["memory"][key].as_u64().is_some(), "missing {key}");
        }
        assert_eq!(json["worlds"][0]["clientCount"], 10);
        assert_eq!(json["worlds"][0]["messageQueueBulk"], 3);
        assert!(!json.to_string().contains("sensitive-world-id"));
    }
}
