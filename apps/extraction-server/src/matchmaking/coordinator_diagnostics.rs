use super::coordinator::Coordinator;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CoordinatorResourceSnapshot {
    pub(crate) queued_accounts: usize,
    pub(crate) connected_accounts: usize,
    pub(crate) connection_routes: usize,
    pub(crate) live_matches: usize,
    pub(crate) pending_settlements: usize,
    pub(crate) pending_despawns: usize,
    pub(crate) hard_deadline_tasks: usize,
    pub(crate) ticker_pending: bool,
    pub(crate) runtime_generations: usize,
    pub(crate) runtime_owned_matches: usize,
    pub(crate) runtime_forced_eliminations: usize,
    pub(crate) runtime_hard_deadlines: usize,
}

impl Coordinator {
    pub(super) fn resource_snapshot(&self) -> CoordinatorResourceSnapshot {
        let runtime = self
            .runtime
            .as_ref()
            .map(|runtime| runtime.resource_snapshot())
            .unwrap_or_default();
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
            ticker_pending: false,
            runtime_generations: runtime.generations,
            runtime_owned_matches: runtime.owned_matches,
            runtime_forced_eliminations: runtime.forced_eliminations,
            runtime_hard_deadlines: runtime.hard_deadlines,
        }
    }
}
