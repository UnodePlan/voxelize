use tokio::time::{sleep, timeout, Duration};
use voxelize::{PrepareWorld, RemoveWorld, WorldLifecycleState};

use super::EngineMatchWorldRuntime;
use crate::ports::MatchWorldRuntimeError;

const WORLD_READY_TIMEOUT: Duration = Duration::from_secs(60);
const WORLD_READY_POLL_INTERVAL: Duration = Duration::from_millis(25);
const WORLD_ROLLBACK_TIMEOUT: Duration = Duration::from_secs(5);

impl EngineMatchWorldRuntime {
    pub(super) async fn wait_until_ready(
        &self,
        world_name: &str,
    ) -> Result<String, MatchWorldRuntimeError> {
        let wait = async {
            let mut expected_generation = None;
            loop {
                let prepared = self
                    .server
                    .send(PrepareWorld {
                        name: world_name.to_owned(),
                        expected_generation: expected_generation.clone(),
                    })
                    .await
                    .map_err(|_| MatchWorldRuntimeError::Unavailable)?
                    .map_err(|_| MatchWorldRuntimeError::Unavailable)?;
                if prepared.lifecycle == WorldLifecycleState::Ready {
                    return Ok(prepared.generation);
                }
                if prepared.lifecycle != WorldLifecycleState::Preparing {
                    return Err(MatchWorldRuntimeError::Unavailable);
                }
                expected_generation = Some(prepared.generation);
                sleep(WORLD_READY_POLL_INTERVAL).await;
            }
        };
        timeout(WORLD_READY_TIMEOUT, wait)
            .await
            .map_err(|_| MatchWorldRuntimeError::Unavailable)?
    }

    pub(super) async fn rollback_world(&self, world_name: &str) {
        let remove = self.server.send(RemoveWorld {
            name: world_name.to_owned(),
        });
        let _ = timeout(WORLD_ROLLBACK_TIMEOUT, remove).await;
        self.generations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(world_name);
        self.owned_matches
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(world_name);
        self.forced_eliminations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(world_name);
    }
}
