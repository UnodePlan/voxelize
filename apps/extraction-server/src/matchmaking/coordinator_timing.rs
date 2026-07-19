use super::{
    coordinator::{repository_error, Coordinator},
    coordinator_lifecycle::{stop_world_once, WORLD_STOP_TIMEOUT},
    MatchState, MatchmakingError, ParticipantState,
};
use crate::observability::{MatchEvent, ObservedMatchPhase};
use crate::ports::SettlingTrigger;
use std::time::Duration;

impl Coordinator {
    pub(super) async fn advance_time(&mut self) -> Result<(), MatchmakingError> {
        self.retry_pending_settlements().await?;
        if self.current.is_none() && self.queue.len() == self.match_size {
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
            self.dispatch_hard_deadline_now()?;
            return Ok(());
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
        let hard_deadline_at = stored
            .record
            .hard_deadline
            .ok_or(MatchmakingError::RosterLocked)?;
        let settlement_grace_deadline = stored
            .record
            .settlement_grace_deadline
            .ok_or(MatchmakingError::RosterLocked)?;
        let extraction_open_deadline = self.monotonic_deadline_for(extraction_open_at)?;
        let hard_deadline = self.monotonic_deadline_for(hard_deadline_at)?;
        let activated_at = self.clock.monotonic_now();
        let hard_deadline_reached = activated_at >= hard_deadline;
        if let Some(current) = self.current.as_mut() {
            current.state = stored.record.state;
            current.activated_at = Some(activated_at);
            current.extraction_open_deadline = Some(extraction_open_deadline);
            current.hard_deadline = Some(hard_deadline);
            current.extraction_open_at_utc = Some(extraction_open_at);
            current.hard_deadline_utc = Some(hard_deadline_at);
            current.settlement_grace_deadline_utc = Some(settlement_grace_deadline);
            for participant in current.participants.values_mut() {
                participant.state = ParticipantState::Active;
            }
        }
        self.sync_gate();
        self.record_event(MatchEvent::PhaseChanged {
            match_id,
            phase: ObservedMatchPhase::Active,
        });
        if hard_deadline_reached {
            self.dispatch_hard_deadline_now()?;
            return Ok(());
        }
        self.arm_hard_deadline_watchdog()
    }

    fn arm_hard_deadline_watchdog(&mut self) -> Result<(), MatchmakingError> {
        let current = self.current.as_mut().ok_or(MatchmakingError::Unavailable)?;
        let hard_deadline = current.hard_deadline.ok_or(MatchmakingError::Unavailable)?;
        let delay = hard_deadline.saturating_sub(self.clock.monotonic_now());
        self.spawn_hard_deadline_watchdog(delay)
    }

    fn dispatch_hard_deadline_now(&mut self) -> Result<(), MatchmakingError> {
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.hard_deadline_closing)
        {
            return Ok(());
        }
        if let Some(task) = self
            .current
            .as_mut()
            .and_then(|current| current.hard_deadline_task.take())
        {
            task.abort();
        }
        if let Some(current) = self.current.as_mut() {
            current.hard_deadline_closing = true;
        }
        self.spawn_hard_deadline_watchdog(Duration::ZERO)
    }

    fn spawn_hard_deadline_watchdog(&mut self, delay: Duration) -> Result<(), MatchmakingError> {
        let current = self.current.as_mut().ok_or(MatchmakingError::Unavailable)?;
        let hard_deadline = current.hard_deadline.ok_or(MatchmakingError::Unavailable)?;
        let hard_deadline_utc = current
            .hard_deadline_utc
            .ok_or(MatchmakingError::Unavailable)?;
        let world_generation = current
            .world_generation
            .clone()
            .ok_or(MatchmakingError::Unavailable)?;
        let match_id = current.match_id;
        let world_name = current.world_name.clone();
        let gate = self.gate.clone();
        let sender = self.sender.clone();
        let runtime = self
            .runtime
            .as_ref()
            .cloned()
            .ok_or(MatchmakingError::Unavailable)?;
        current.hard_deadline_task = Some(tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let owns_gate = gate.close_for_hard_deadline(&world_name, &world_generation);
            let (sealed, world_stopped) = if owns_gate {
                // `Ok(false)` 表示运行时明确未完成封口，不能继续进入结算终态。
                let sealed = matches!(
                    runtime
                        .seal_hard_deadline(&world_name, hard_deadline, hard_deadline_utc)
                        .await,
                    Ok(true)
                );
                let world_stopped =
                    stop_world_once(runtime.as_ref(), match_id, &world_name, WORLD_STOP_TIMEOUT)
                        .await
                        .is_ok();
                (sealed, world_stopped)
            } else {
                (false, false)
            };
            let _ = sender
                .send(super::command::Command::HardDeadlineSealed {
                    match_id,
                    world_name,
                    world_generation,
                    sealed,
                    world_stopped,
                })
                .await;
        }));
        Ok(())
    }

    pub(super) async fn complete_hard_deadline_seal(
        &mut self,
        match_id: uuid::Uuid,
        world_name: &str,
        world_generation: &str,
        sealed: bool,
        world_stopped: bool,
    ) -> Result<(), MatchmakingError> {
        let Some(current) = self.current.as_mut() else {
            return Ok(());
        };
        if current.match_id != match_id
            || current.world_name != world_name
            || current.world_generation.as_deref() != Some(world_generation)
        {
            return Ok(());
        }
        if !matches!(
            current.state,
            MatchState::Active | MatchState::ExtractionOpen
        ) {
            return Ok(());
        }
        current.hard_deadline_task = None;
        current.hard_deadline_closing = false;
        current.world_stopped |= world_stopped;
        if !sealed {
            return Err(MatchmakingError::Unavailable);
        }
        self.begin_settling(SettlingTrigger::HardDeadline).await?;
        self.complete_settling_progress().await
    }
}
