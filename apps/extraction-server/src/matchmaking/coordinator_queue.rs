use std::collections::HashMap;

use uuid::Uuid;

use super::{
    coordinator::{repository_error, Coordinator, LiveMatch, LiveParticipant, QueueEntry},
    CreatePreparingMatch, FrozenRoster, MatchState, MatchmakingError, ParticipantState,
    QueueSnapshot, QueuedPlayer,
};
use crate::observability::{MatchEvent, ObservedMatchPhase};
use crate::ports::MatchWorldRuntimeError;

impl Coordinator {
    pub(super) async fn enqueue(
        &mut self,
        account_id: Uuid,
    ) -> Result<QueueSnapshot, MatchmakingError> {
        if self.runtime.is_none() {
            return Err(MatchmakingError::Unavailable);
        }
        if self
            .queue
            .iter()
            .any(|entry| entry.account_id == account_id)
        {
            return if self.current.is_none() && self.queue.len() == self.match_size {
                self.prepare_first_roster(account_id).await
            } else {
                self.snapshot_for(account_id)
                    .ok_or(MatchmakingError::Unavailable)
            };
        }
        if let Some(current) = self.current.as_ref() {
            match current.participants.get(&account_id) {
                Some(participant) if participant.state.is_terminal() => {}
                Some(_) => return Ok(current.snapshot()),
                None => return Err(MatchmakingError::Full),
            }
        }
        if !self.is_connected(account_id) {
            return Err(MatchmakingError::ConnectionRequired);
        }
        if self.queue.len() >= self.match_size {
            return Err(MatchmakingError::Full);
        }
        if self
            .repository
            .find_nonterminal_by_account(account_id)
            .await
            .map_err(repository_error)?
            .is_some()
        {
            return Err(MatchmakingError::RosterLocked);
        }

        let order = self.next_order;
        self.next_order = self
            .next_order
            .checked_add(1)
            .ok_or(MatchmakingError::Unavailable)?;
        self.queue.push_back(QueueEntry {
            account_id,
            enqueued_at: self.utc_now(),
            order,
        });
        if self.current.is_none() && self.queue.len() == self.match_size {
            self.prepare_first_roster(account_id).await
        } else {
            self.snapshot_for(account_id)
                .ok_or(MatchmakingError::Unavailable)
        }
    }

    pub(super) async fn cancel(
        &mut self,
        account_id: Uuid,
    ) -> Result<QueueSnapshot, MatchmakingError> {
        if self.current.as_ref().is_some_and(|item| {
            item.participants
                .get(&account_id)
                .is_some_and(|participant| !participant.state.is_terminal())
        }) {
            return Err(MatchmakingError::RosterLocked);
        }
        let Some(index) = self
            .queue
            .iter()
            .position(|entry| entry.account_id == account_id)
        else {
            return Ok(QueueSnapshot::idle(false));
        };
        self.abort_pending_prepare("queue_cancelled_before_prepare_retry")
            .await?;
        self.queue.remove(index);
        Ok(QueueSnapshot::idle(true))
    }

    pub(super) async fn abort_pending_prepare(
        &mut self,
        reason: &str,
    ) -> Result<(), MatchmakingError> {
        let Some(command) = self.prepare_attempt.as_ref() else {
            return Ok(());
        };
        let match_id = command.match_id;
        if self
            .repository
            .find_match(match_id)
            .await
            .map_err(repository_error)?
            .is_some_and(|stored| !stored.record.state.is_terminal())
        {
            self.repository
                .abort(match_id, reason.to_owned(), self.utc_now())
                .await
                .map_err(repository_error)?;
        }
        self.prepare_attempt = None;
        Ok(())
    }

    pub(super) async fn prepare_first_roster(
        &mut self,
        requesting_account: Uuid,
    ) -> Result<QueueSnapshot, MatchmakingError> {
        let runtime = self
            .runtime
            .as_ref()
            .cloned()
            .ok_or(MatchmakingError::Unavailable)?;
        let capacity = self.match_size;
        let original_queue = self
            .queue
            .iter()
            .take(capacity)
            .cloned()
            .collect::<Vec<_>>();
        let command = match &self.prepare_attempt {
            Some(command) => command.clone(),
            None => {
                let players: Vec<QueuedPlayer> = original_queue
                    .iter()
                    .map(|entry| QueuedPlayer {
                        account_id: entry.account_id,
                        public_player_id: self.ids.next_uuid(),
                        enqueued_at: entry.enqueued_at,
                    })
                    .collect();
                let roster =
                    FrozenRoster::try_from(players).map_err(|_| MatchmakingError::Unavailable)?;
                let match_id = self.ids.next_uuid();
                let command = CreatePreparingMatch {
                    match_id,
                    world_name: format!("match-{}", match_id.simple()),
                    seed: self.seeds.next_seed(),
                    versions: self.versions.clone(),
                    created_at: self.utc_now(),
                    roster,
                };
                self.prepare_attempt = Some(command.clone());
                command
            }
        };
        let match_id = command.match_id;
        let world_name = command.world_name.clone();
        let roster = command.roster.clone();
        let stored = self
            .repository
            .create_preparing(command)
            .await
            .map_err(repository_error)?;
        self.prepare_attempt = None;

        self.queue.drain(..capacity);
        let participants = roster
            .iter()
            .map(|participant| {
                (
                    participant.account_id,
                    LiveParticipant {
                        public_player_id: participant.public_player_id,
                        state: ParticipantState::Preparing,
                        joined: false,
                        control_connection: None,
                        reconnect_deadline: None,
                        despawn_pending: false,
                    },
                )
            })
            .collect::<HashMap<_, _>>();
        self.current = Some(LiveMatch {
            match_id,
            world_name: world_name.clone(),
            world_generation: None,
            state: MatchState::Preparing,
            original_queue: original_queue.clone(),
            participants,
            extraction_open_deadline: None,
            hard_deadline: None,
            extraction_open_at_utc: None,
            hard_deadline_utc: None,
            settlement_grace_deadline_utc: None,
            activated_at: None,
            abort_reason: None,
            settling_trigger: None,
            settling_persisted: false,
            world_stopped: false,
            hard_deadline_task: None,
            hard_deadline_closing: false,
        });
        self.sync_gate();

        let spec = crate::ports::MatchWorldSpec::from_preparing(&stored.record, roster);
        let prepared = match runtime.prepare_world(spec).await {
            Ok(prepared) => prepared,
            Err(error) => return self.rollback_failed_prepare(error).await,
        };
        if let Some(current) = self.current.as_mut() {
            current.world_generation = Some(prepared.world_generation);
        }
        self.sync_gate();
        self.record_event(MatchEvent::PhaseChanged {
            match_id,
            phase: ObservedMatchPhase::Preparing,
        });
        self.snapshot_for(requesting_account)
            .ok_or(MatchmakingError::Unavailable)
    }

    async fn rollback_failed_prepare(
        &mut self,
        error: MatchWorldRuntimeError,
    ) -> Result<QueueSnapshot, MatchmakingError> {
        self.abort_current("world_prepare_failed").await?;
        Err(match error {
            MatchWorldRuntimeError::Conflict => MatchmakingError::RosterLocked,
            MatchWorldRuntimeError::Unavailable => MatchmakingError::Unavailable,
        })
    }
}
