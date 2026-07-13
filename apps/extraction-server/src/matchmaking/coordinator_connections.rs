use std::time::Duration;

use uuid::Uuid;

use super::{
    coordinator::{repository_error, Coordinator},
    MatchState, MatchmakingError, ParticipantState,
};

impl Coordinator {
    pub(super) fn add_connection(&mut self, account_id: Uuid, connection_id: String) {
        self.connections
            .entry(account_id)
            .or_default()
            .insert(connection_id);
    }

    pub(super) async fn join_committed(
        &mut self,
        account_id: Uuid,
        connection_id: String,
        world_name: &str,
        world_generation: &str,
        client_id: &str,
        attach_attempt_id: &str,
    ) -> Result<(), MatchmakingError> {
        if !self.event_matches(account_id, world_name, world_generation, client_id) {
            return Ok(());
        }
        if !self.gate.take_join_reservation(
            world_name,
            world_generation,
            client_id,
            attach_attempt_id,
            account_id,
        ) {
            return Ok(());
        }
        let should_activate = {
            let Some(current) = self.current.as_mut() else {
                return Ok(());
            };
            if current.world_name != world_name || current.state != MatchState::Preparing {
                return Ok(());
            }
            let Some(participant) = current.participants.get_mut(&account_id) else {
                return Ok(());
            };
            participant.joined = true;
            participant.control_connection = Some(connection_id);
            current
                .participants
                .values()
                .all(|participant| participant.joined)
        };
        self.sync_gate();
        if should_activate {
            self.activate().await?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) async fn disconnected(
        &mut self,
        account_id: Uuid,
        connection_id: &str,
        observed_at: Duration,
        world_name: Option<&str>,
        world_generation: Option<&str>,
        client_id: Option<&str>,
        attach_attempt_id: Option<&str>,
    ) -> Result<(), MatchmakingError> {
        let removed = self
            .connections
            .get_mut(&account_id)
            .is_some_and(|connections| connections.remove(connection_id));
        if !removed {
            return Ok(());
        }
        if self
            .connections
            .get(&account_id)
            .is_some_and(|connections| connections.is_empty())
        {
            self.connections.remove(&account_id);
        }

        if let (
            Some(world_name),
            Some(world_generation),
            Some(client_id),
            Some(attach_attempt_id),
        ) = (world_name, world_generation, client_id, attach_attempt_id)
        {
            self.gate.clear_attach_reservation(
                world_name,
                world_generation,
                client_id,
                attach_attempt_id,
                account_id,
            );
        }
        let event_matches = match (world_name, world_generation, client_id) {
            (Some(world_name), Some(world_generation), Some(client_id)) => {
                self.event_matches(account_id, world_name, world_generation, client_id)
            }
            (None, None, None) => true,
            _ => false,
        };
        let control_disconnected = event_matches
            && self.current.as_ref().is_some_and(|current| {
                current
                    .participants
                    .get(&account_id)
                    .and_then(|participant| participant.control_connection.as_deref())
                    == Some(connection_id)
            });
        let carries_match_identity = world_name.is_some()
            && world_generation.is_some()
            && client_id.is_some()
            && event_matches;
        let preparing_participant_disconnected = (carries_match_identity
            || !self.is_connected(account_id))
            && self.current.as_ref().is_some_and(|current| {
                current.state == MatchState::Preparing
                    && current.participants.contains_key(&account_id)
            });
        if preparing_participant_disconnected {
            self.abort_current("preparing_participant_disconnected")
                .await?;
            return Ok(());
        }
        let disconnected_before_activation = (carries_match_identity
            || !self.is_connected(account_id))
            && self.current.as_ref().is_some_and(|current| {
                current.state == MatchState::Active
                    && current
                        .activated_at
                        .is_some_and(|activated_at| observed_at < activated_at)
                    && current.participants.contains_key(&account_id)
            });
        if disconnected_before_activation {
            self.abort_current("participant_disconnected_before_activation")
                .await?;
            return Ok(());
        }
        if !self.is_connected(account_id)
            && self
                .queue
                .iter()
                .any(|entry| entry.account_id == account_id)
        {
            self.abort_pending_prepare("queued_participant_disconnected_before_prepare_retry")
                .await?;
            self.queue.retain(|entry| entry.account_id != account_id);
        }
        if (!self.is_connected(account_id) || control_disconnected)
            && event_matches
            && matches!(
                self.current.as_ref().map(|current| current.state),
                Some(MatchState::Active | MatchState::ExtractionOpen)
            )
            && self
                .current
                .as_ref()
                .and_then(|current| current.participants.get(&account_id))
                .is_some_and(|participant| participant.state == ParticipantState::Active)
        {
            self.mark_disconnected(account_id).await?;
        }
        Ok(())
    }

    pub(super) async fn rebound(
        &mut self,
        account_id: Uuid,
        connection_id: String,
        world_name: &str,
        world_generation: &str,
        client_id: &str,
        attach_attempt_id: &str,
    ) -> Result<(), MatchmakingError> {
        if !self.event_matches(account_id, world_name, world_generation, client_id) {
            return Ok(());
        }
        let Some(admitted_at) = self.gate.take_rebind_reservation(
            world_name,
            world_generation,
            client_id,
            attach_attempt_id,
            account_id,
        ) else {
            return Ok(());
        };
        let Some(current) = self.current.as_ref() else {
            return Ok(());
        };
        if current.world_name != world_name {
            return Ok(());
        }
        let Some(participant) = current.participants.get(&account_id) else {
            return Ok(());
        };
        if participant.state != ParticipantState::Disconnected {
            return Ok(());
        }
        let match_id = current.match_id;
        self.repository
            .reconnect(match_id, account_id, admitted_at)
            .await
            .map_err(repository_error)?;
        if let Some(participant) = self
            .current
            .as_mut()
            .and_then(|current| current.participants.get_mut(&account_id))
        {
            participant.state = ParticipantState::Active;
            participant.control_connection = Some(connection_id);
            participant.reconnect_deadline = None;
        }
        self.sync_gate();
        Ok(())
    }

    fn event_matches(
        &self,
        account_id: Uuid,
        world_name: &str,
        world_generation: &str,
        client_id: &str,
    ) -> bool {
        self.current.as_ref().is_some_and(|current| {
            current.world_name == world_name
                && current.world_generation.as_deref() == Some(world_generation)
                && current
                    .participants
                    .get(&account_id)
                    .is_some_and(|participant| {
                        participant.public_player_id.to_string() == client_id
                    })
        })
    }
}
