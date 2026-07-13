use time::OffsetDateTime;
use uuid::Uuid;

use super::EngineMatchWorldRuntime;
use crate::{engine_gameplay::HardDeadlineRequest, ports::MatchWorldRuntimeError};

impl EngineMatchWorldRuntime {
    pub(super) fn world_generation(&self, world_name: &str) -> Option<String> {
        self.generations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(world_name)
            .cloned()
    }

    pub(super) fn queue_timeout_elimination(
        &self,
        world_name: &str,
        account_id: Uuid,
    ) -> Result<bool, MatchWorldRuntimeError> {
        let queues = self
            .forced_eliminations
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(queue) = queues.get(world_name) else {
            return Ok(false);
        };
        queue
            .enqueue(account_id)
            .map_err(|_| MatchWorldRuntimeError::Unavailable)
    }

    pub(super) async fn wait_for_hard_deadline_seal(
        &self,
        world_name: &str,
        monotonic_deadline: std::time::Duration,
        utc_deadline: OffsetDateTime,
    ) -> Result<bool, MatchWorldRuntimeError> {
        let control = self
            .hard_deadlines
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(world_name)
            .cloned()
            .ok_or(MatchWorldRuntimeError::Conflict)?;
        let request = HardDeadlineRequest {
            monotonic_deadline,
            utc_deadline,
        };
        if !control.request(request) {
            return Err(MatchWorldRuntimeError::Conflict);
        }
        let wait = async {
            loop {
                if control.is_sealed(request) {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        };
        tokio::time::timeout(std::time::Duration::from_secs(2), wait)
            .await
            .map_err(|_| MatchWorldRuntimeError::Unavailable)?;
        Ok(true)
    }
}
