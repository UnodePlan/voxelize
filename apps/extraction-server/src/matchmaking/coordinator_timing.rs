use super::{
    coordinator::{repository_error, Coordinator},
    coordinator_lifecycle::{stop_world_once, WORLD_STOP_TIMEOUT},
    MatchState, MatchmakingError, ParticipantState,
};
use crate::ports::SettlingTrigger;

impl Coordinator {
    pub(super) async fn advance_time(&mut self) -> Result<(), MatchmakingError> {
        if self.current.is_none() && self.queue.len() == super::MATCH_SIZE {
            if self.gate.is_failed_closed() {
                return Err(MatchmakingError::Unavailable);
            }
            let requesting_account = self
                .queue
                .front()
                .map(|entry| entry.account_id)
                .ok_or(MatchmakingError::Unavailable)?;
            self.prepare_first_roster(requesting_account).await?;
            return Ok(());
        }
        let now = self.clock.monotonic_now();
        let state = self.current.as_ref().map(|current| current.state);
        if state == Some(MatchState::Aborted) {
            return self.complete_abort().await;
        }
        if state == Some(MatchState::Finished) {
            return self.complete_finish().await;
        }
        if state == Some(MatchState::Settling) {
            return self.complete_settling_progress().await;
        }
        if matches!(state, Some(MatchState::Active | MatchState::ExtractionOpen))
            && self
                .current
                .as_ref()
                .and_then(|current| current.hard_deadline)
                .is_some_and(|deadline| now >= deadline)
        {
            self.begin_settling(SettlingTrigger::HardDeadline).await?;
            return self.complete_settling_progress().await;
        }
        let timeout_world = self.current.as_ref().and_then(|current| {
            current
                .world_generation
                .as_ref()
                .map(|generation| (current.world_name.clone(), generation.clone()))
        });
        let expired = self
            .current
            .as_ref()
            .map(|current| {
                current
                    .participants
                    .iter()
                    .filter(|(_, participant)| {
                        participant.state == ParticipantState::Disconnected
                            && participant
                                .reconnect_deadline
                                .is_some_and(|deadline| now >= deadline)
                    })
                    .map(|(account_id, _)| *account_id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for account_id in expired {
            let claimed = timeout_world
                .as_ref()
                .is_none_or(|(world_name, world_generation)| {
                    self.gate
                        .claim_rebind_timeout(world_name, world_generation, account_id)
                });
            if !claimed {
                continue;
            }
            let result = self.time_out(account_id).await;
            if let Some((world_name, world_generation)) = timeout_world.as_ref() {
                self.gate
                    .release_rebind_timeout_claim(world_name, world_generation, account_id);
            }
            result?;
        }
        self.retry_pending_despawns().await?;

        let state = self.current.as_ref().map(|current| current.state);
        if matches!(state, Some(MatchState::Active))
            && self
                .current
                .as_ref()
                .and_then(|current| current.extraction_open_deadline)
                .is_some_and(|deadline| now >= deadline)
        {
            self.open_extraction().await?;
        }
        if self.current.as_ref().is_some_and(|current| {
            matches!(
                current.state,
                MatchState::Active | MatchState::ExtractionOpen
            ) && current
                .participants
                .values()
                .all(|participant| participant.state.is_terminal())
        }) {
            self.begin_settling(SettlingTrigger::AllParticipantsTerminal)
                .await?;
            return self.complete_settling_progress().await;
        }
        Ok(())
    }

    async fn complete_settling_progress(&mut self) -> Result<(), MatchmakingError> {
        if self
            .current
            .as_ref()
            .is_some_and(|current| !current.settling_persisted)
        {
            let trigger = self
                .current
                .as_ref()
                .and_then(|current| current.settling_trigger)
                .ok_or(MatchmakingError::Unavailable)?;
            self.begin_settling(trigger).await?;
        }
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.settling_persisted)
        {
            self.finish().await?;
        }
        Ok(())
    }

    pub(super) async fn activate(&mut self) -> Result<(), MatchmakingError> {
        let Some(match_id) = self.current.as_ref().map(|current| current.match_id) else {
            return Ok(());
        };
        let stored = self
            .repository
            .activate(match_id, self.utc_now())
            .await
            .map_err(repository_error)?
            .into_value();
        let extraction_open_at = stored
            .record
            .extraction_open_at
            .ok_or(MatchmakingError::RosterLocked)?;
        let hard_deadline = stored
            .record
            .hard_deadline
            .ok_or(MatchmakingError::RosterLocked)?;
        let extraction_open_deadline = self.monotonic_deadline_for(extraction_open_at)?;
        let hard_deadline = self.monotonic_deadline_for(hard_deadline)?;
        let activated_at = self.clock.monotonic_now();
        let hard_deadline_reached = activated_at >= hard_deadline;
        if let Some(current) = self.current.as_mut() {
            current.state = if hard_deadline_reached {
                MatchState::Settling
            } else {
                stored.record.state
            };
            current.activated_at = Some(activated_at);
            current.extraction_open_deadline = Some(extraction_open_deadline);
            current.hard_deadline = Some(hard_deadline);
            for participant in current.participants.values_mut() {
                participant.state = ParticipantState::Active;
            }
        }
        self.sync_gate();
        if hard_deadline_reached {
            return self.begin_settling(SettlingTrigger::HardDeadline).await;
        }
        self.arm_hard_deadline_watchdog()
    }

    fn arm_hard_deadline_watchdog(&mut self) -> Result<(), MatchmakingError> {
        let current = self.current.as_mut().ok_or(MatchmakingError::Unavailable)?;
        let hard_deadline = current.hard_deadline.ok_or(MatchmakingError::Unavailable)?;
        let world_generation = current
            .world_generation
            .clone()
            .ok_or(MatchmakingError::Unavailable)?;
        let delay = hard_deadline.saturating_sub(self.clock.monotonic_now());
        let match_id = current.match_id;
        let world_name = current.world_name.clone();
        let gate = self.gate.clone();
        let runtime = self
            .runtime
            .as_ref()
            .cloned()
            .ok_or(MatchmakingError::Unavailable)?;
        current.hard_deadline_task = Some(tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            if gate.close_for_hard_deadline(&world_name, &world_generation) {
                for retry in 0..3 {
                    if stop_world_once(runtime.as_ref(), match_id, &world_name, WORLD_STOP_TIMEOUT)
                        .await
                        .is_ok()
                    {
                        break;
                    }
                    if retry < 2 {
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }
        }));
        Ok(())
    }
}
