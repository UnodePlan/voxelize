use std::sync::{Arc, Weak};

use uuid::Uuid;
use voxelize::{ConnectionLifecycleEvent, ConnectionLifecycleObserver};

use crate::matchmaking::{MatchConnectionEvent, MatchmakingService};

pub(crate) struct MatchConnectionObserver {
    matchmaking: Weak<MatchmakingService>,
}

impl MatchConnectionObserver {
    pub(crate) fn new(matchmaking: &Arc<MatchmakingService>) -> Self {
        Self {
            matchmaking: Arc::downgrade(matchmaking),
        }
    }
}

impl ConnectionLifecycleObserver for MatchConnectionObserver {
    fn observe(&self, event: &ConnectionLifecycleEvent) {
        let Some(service) = self.matchmaking.upgrade() else {
            return;
        };
        let mapped = match event {
            ConnectionLifecycleEvent::Connected {
                connection_id,
                principal: Some(principal),
            } => map_account(&service, principal.account_id.as_str()).map(|account_id| {
                MatchConnectionEvent::Connected {
                    connection_id: connection_id.clone(),
                    account_id,
                }
            }),
            ConnectionLifecycleEvent::JoinCommitted {
                connection_id,
                principal: Some(principal),
                world_name,
                world_generation,
                client_id,
                attach_attempt_id,
            } => map_account(&service, principal.account_id.as_str()).map(|account_id| {
                MatchConnectionEvent::JoinCommitted {
                    connection_id: connection_id.clone(),
                    account_id,
                    world_name: world_name.clone(),
                    world_generation: world_generation.clone(),
                    client_id: client_id.clone(),
                    attach_attempt_id: attach_attempt_id.clone(),
                }
            }),
            ConnectionLifecycleEvent::Disconnected {
                connection_id,
                principal: Some(principal),
                world_name,
                world_generation,
                client_id,
                attach_attempt_id,
            } => map_account(&service, principal.account_id.as_str()).map(|account_id| {
                MatchConnectionEvent::Disconnected {
                    connection_id: connection_id.clone(),
                    account_id,
                    observed_at: service.monotonic_now(),
                    world_name: world_name.clone(),
                    world_generation: world_generation.clone(),
                    client_id: client_id.clone(),
                    attach_attempt_id: attach_attempt_id.clone(),
                }
            }),
            ConnectionLifecycleEvent::Detached {
                principal,
                world_name,
                world_generation,
                client_id,
                attach_attempt_id,
                ..
            } => map_account(&service, principal.account_id.as_str()).map(|account_id| {
                MatchConnectionEvent::Detached {
                    account_id,
                    world_name: world_name.clone(),
                    world_generation: world_generation.clone(),
                    client_id: client_id.clone(),
                    attach_attempt_id: attach_attempt_id.clone(),
                }
            }),
            ConnectionLifecycleEvent::Rebound {
                connection_id,
                principal,
                world_name,
                world_generation,
                client_id,
                attach_attempt_id,
            } => map_account(&service, principal.account_id.as_str()).map(|account_id| {
                MatchConnectionEvent::Rebound {
                    connection_id: connection_id.clone(),
                    account_id,
                    world_name: world_name.clone(),
                    world_generation: world_generation.clone(),
                    client_id: client_id.clone(),
                    attach_attempt_id: attach_attempt_id.clone(),
                }
            }),
            ConnectionLifecycleEvent::RebindRejected {
                principal,
                world_name,
                world_generation,
                client_id,
                attach_attempt_id,
            } => map_account(&service, principal.account_id.as_str()).map(|account_id| {
                MatchConnectionEvent::RebindRejected {
                    account_id,
                    world_name: world_name.clone(),
                    world_generation: world_generation.clone(),
                    client_id: client_id.clone(),
                    attach_attempt_id: attach_attempt_id.clone(),
                }
            }),
            _ => None,
        };
        if let Some(event) = mapped {
            service.observe_connection_event(event);
        }
    }
}

fn map_account(service: &MatchmakingService, value: &str) -> Option<Uuid> {
    match Uuid::parse_str(value) {
        Ok(account_id) => Some(account_id),
        Err(_) => {
            service.fail_closed();
            None
        }
    }
}
