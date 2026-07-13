use super::coordinator::Coordinator;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct CoordinatorResourceSnapshot {
    pub queued_accounts: usize,
    pub connected_accounts: usize,
    pub connection_routes: usize,
    pub live_matches: usize,
    pub pending_settlements: usize,
    pub pending_despawns: usize,
    pub hard_deadline_tasks: usize,
}

impl Coordinator {
    pub(super) fn resource_snapshot(&self) -> CoordinatorResourceSnapshot {
        CoordinatorResourceSnapshot {
            queued_accounts: self.queue.len(),
            connected_accounts: self.connections.len(),
            connection_routes: self.connections.values().map(|routes| routes.len()).sum(),
            live_matches: usize::from(self.current.is_some()),
            pending_settlements: self.pending_settlements.len(),
            pending_despawns: self
                .current
                .iter()
                .flat_map(|current| current.participants.values())
                .filter(|participant| participant.despawn_pending)
                .count(),
            hard_deadline_tasks: usize::from(
                self.current
                    .as_ref()
                    .is_some_and(|current| current.hard_deadline_task.is_some()),
            ),
        }
    }
}
