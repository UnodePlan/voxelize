use std::time::Duration;

use super::{
    coordinator::{repository_error, runtime_error, Coordinator},
    MatchState, MatchmakingError, ParticipantState,
};
use crate::ports::{MatchWorldRuntime, SettlingTrigger};

pub(super) const WORLD_STOP_TIMEOUT: Duration = Duration::from_secs(2);

pub(super) async fn stop_world_once(
    runtime: &dyn MatchWorldRuntime,
    match_id: uuid::Uuid,
    world_name: &str,
    timeout: Duration,
) -> Result<(), MatchmakingError> {
    tokio::time::timeout(timeout, runtime.stop_world(match_id, world_name))
        .await
        .map_err(|_| MatchmakingError::Unavailable)?
        .map_err(runtime_error)?;
    Ok(())
}

impl Coordinator {
    pub(super) async fn open_extraction(&mut self) -> Result<(), MatchmakingError> {
        let Some(match_id) = self.current.as_ref().map(|current| current.match_id) else {
            return Ok(());
        };
        let stored = self
            .repository
            .open_extraction(match_id, self.utc_now())
            .await
            .map_err(repository_error)?
            .into_value();
        if let Some(current) = self.current.as_mut() {
            current.state = stored.record.state;
        }
        self.sync_gate();
        Ok(())
    }

    pub(super) async fn begin_settling(
        &mut self,
        trigger: SettlingTrigger,
    ) -> Result<(), MatchmakingError> {
        let Some((match_id, world_name)) = self
            .current
            .as_ref()
            .map(|current| (current.match_id, current.world_name.clone()))
        else {
            return Ok(());
        };
        if let Some(current) = self.current.as_mut() {
            current.state = MatchState::Settling;
            current.settling_trigger.get_or_insert(trigger);
            if let Some(task) = current.hard_deadline_task.take() {
                task.abort();
            }
        }
        self.sync_gate();
        if self
            .current
            .as_ref()
            .is_some_and(|current| !current.world_stopped)
        {
            stop_world_once(
                self.runtime
                    .as_deref()
                    .ok_or(MatchmakingError::Unavailable)?,
                match_id,
                &world_name,
                WORLD_STOP_TIMEOUT,
            )
            .await?;
            if let Some(current) = self.current.as_mut() {
                current.world_stopped = true;
            }
        }
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.settling_persisted)
        {
            return Ok(());
        }
        let stored = self
            .repository
            .begin_settling(match_id, trigger, self.utc_now())
            .await
            .map_err(repository_error)?
            .into_value();
        if let Some(current) = self.current.as_mut() {
            current.state = stored.record.state;
            current.settling_persisted = true;
            for record in stored.participants {
                if let Some(participant) = current.participants.get_mut(&record.account_id) {
                    participant.state = record.state;
                    if record.state.is_terminal() {
                        participant.control_connection = None;
                        participant.reconnect_deadline = None;
                    }
                }
            }
        }
        self.sync_gate();
        Ok(())
    }

    pub(super) async fn finish(&mut self) -> Result<(), MatchmakingError> {
        let Some(match_id) = self.current.as_ref().map(|current| current.match_id) else {
            return Ok(());
        };
        let stored = self
            .repository
            .finish(match_id, self.utc_now())
            .await
            .map_err(repository_error)?
            .into_value();
        if let Some(current) = self.current.as_mut() {
            current.state = stored.record.state;
        }
        self.sync_gate();
        self.complete_finish().await
    }

    pub(super) async fn complete_finish(&mut self) -> Result<(), MatchmakingError> {
        let Some((match_id, world_name)) = self
            .current
            .as_ref()
            .filter(|current| current.state == MatchState::Finished)
            .map(|current| (current.match_id, current.world_name.clone()))
        else {
            return Ok(());
        };
        if self
            .current
            .as_ref()
            .is_some_and(|current| !current.world_stopped)
        {
            stop_world_once(
                self.runtime
                    .as_deref()
                    .ok_or(MatchmakingError::Unavailable)?,
                match_id,
                &world_name,
                WORLD_STOP_TIMEOUT,
            )
            .await?;
        }
        if let Some(current) = self.current.as_mut() {
            if let Some(task) = current.hard_deadline_task.take() {
                task.abort();
            }
        }
        self.current = None;
        self.sync_gate();
        Ok(())
    }

    pub(super) async fn abort_current(&mut self, reason: &str) -> Result<(), MatchmakingError> {
        let Some(current) = self.current.as_mut() else {
            return Ok(());
        };
        if current.state == MatchState::Finished {
            return self.complete_finish().await;
        }
        current.state = MatchState::Aborted;
        current.abort_reason = Some(reason.to_owned());
        if let Some(task) = current.hard_deadline_task.take() {
            task.abort();
        }
        for participant in current.participants.values_mut() {
            if !participant.state.is_terminal() {
                participant.state = ParticipantState::Aborted;
            }
            participant.control_connection = None;
            participant.reconnect_deadline = None;
        }
        self.sync_gate();
        self.complete_abort().await
    }

    pub(super) async fn complete_abort(&mut self) -> Result<(), MatchmakingError> {
        let Some((match_id, world_name, reason)) = self
            .current
            .as_ref()
            .filter(|current| current.state == MatchState::Aborted)
            .map(|current| {
                (
                    current.match_id,
                    current.world_name.clone(),
                    current
                        .abort_reason
                        .clone()
                        .unwrap_or_else(|| "match_aborted".to_owned()),
                )
            })
        else {
            return Ok(());
        };
        if self
            .current
            .as_ref()
            .is_some_and(|current| !current.world_stopped)
        {
            stop_world_once(
                self.runtime
                    .as_deref()
                    .ok_or(MatchmakingError::Unavailable)?,
                match_id,
                &world_name,
                WORLD_STOP_TIMEOUT,
            )
            .await?;
            if let Some(current) = self.current.as_mut() {
                current.world_stopped = true;
            }
        }
        self.repository
            .abort(match_id, reason, self.utc_now())
            .await
            .map_err(repository_error)?;
        self.pending_settlements
            .retain(|(pending_match_id, _), _| *pending_match_id != match_id);
        let waiting = self
            .current
            .take()
            .map(|current| current.original_queue)
            .unwrap_or_default();
        self.restore_waiting(waiting);
        self.sync_gate();
        Ok(())
    }
}
