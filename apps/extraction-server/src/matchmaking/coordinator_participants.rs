use uuid::Uuid;

#[cfg(not(test))]
const PARTICIPANT_EVICTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);
#[cfg(test)]
const PARTICIPANT_EVICTION_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(10);

use super::{
    coordinator::{repository_error, runtime_error, Coordinator},
    MatchmakingError,
};

impl Coordinator {
    pub(super) async fn mark_disconnected(
        &mut self,
        account_id: Uuid,
    ) -> Result<(), MatchmakingError> {
        let Some(match_id) = self.current.as_ref().map(|current| current.match_id) else {
            return Ok(());
        };
        let record = self
            .repository
            .mark_disconnected(match_id, account_id, self.utc_now())
            .await
            .map_err(repository_error)?
            .into_value();
        let reconnect_deadline = record
            .reconnect_deadline
            .ok_or(MatchmakingError::RosterLocked)?;
        let deadline = self.monotonic_deadline_for(reconnect_deadline)?;
        if let Some(participant) = self
            .current
            .as_mut()
            .and_then(|current| current.participants.get_mut(&account_id))
        {
            participant.state = record.state;
            participant.control_connection = None;
            participant.reconnect_deadline = Some(deadline);
        }
        self.sync_gate();
        Ok(())
    }

    pub(super) async fn time_out(&mut self, account_id: Uuid) -> Result<(), MatchmakingError> {
        let Some(world_name) = self
            .current
            .as_ref()
            .map(|current| current.world_name.clone())
        else {
            return Ok(());
        };
        let runtime = self.runtime.as_ref().ok_or(MatchmakingError::Unavailable)?;
        runtime
            .request_timeout_elimination(&world_name, account_id)
            .await
            .map_err(runtime_error)?;
        Ok(())
    }

    pub(super) async fn retry_pending_despawns(&mut self) -> Result<(), MatchmakingError> {
        let Some(current) = self.current.as_ref() else {
            return Ok(());
        };
        let world_name = current.world_name.clone();
        let accounts = current
            .participants
            .iter()
            .filter(|(_, participant)| participant.despawn_pending)
            .map(|(account_id, _)| *account_id)
            .collect::<Vec<_>>();
        for account_id in accounts {
            self.evict_one(&world_name, account_id).await?;
        }
        Ok(())
    }

    pub(super) async fn evict_one(
        &mut self,
        world_name: &str,
        account_id: Uuid,
    ) -> Result<(), MatchmakingError> {
        let runtime = self.runtime.as_ref().ok_or(MatchmakingError::Unavailable)?;
        tokio::time::timeout(
            PARTICIPANT_EVICTION_TIMEOUT,
            runtime.evict_participant(world_name, account_id),
        )
        .await
        .map_err(|_| MatchmakingError::Unavailable)?
        .map_err(runtime_error)?;
        if let Some(participant) = self
            .current
            .as_mut()
            .and_then(|current| current.participants.get_mut(&account_id))
        {
            participant.despawn_pending = false;
        }
        Ok(())
    }
}
