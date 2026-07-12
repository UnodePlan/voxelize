use uuid::Uuid;

use super::{gate::AttachGate, MatchState, ParticipantState};

impl AttachGate {
    pub(super) fn public_player_id(&self, world_name: &str, account_id: Uuid) -> Option<String> {
        if self.is_failed_closed() {
            return None;
        }
        let snapshots = self
            .snapshot
            .read()
            .unwrap_or_else(|error| error.into_inner());
        let snapshot = snapshots.as_ref().filter(|snapshot| {
            snapshot.world_name == world_name
                && snapshot.world_generation.is_some()
                && snapshot.state == MatchState::Preparing
        })?;
        let participant = snapshot
            .participants
            .get(&account_id)
            .filter(|participant| {
                participant.state == ParticipantState::Preparing && !participant.joined
            })?;
        Some(participant.public_player_id.to_string())
    }

    pub(super) fn allows_gameplay(
        &self,
        world_name: &str,
        world_generation: &str,
        client_id: &str,
        account_id: Uuid,
    ) -> bool {
        if self.is_failed_closed() {
            return false;
        }
        let snapshots = self
            .snapshot
            .read()
            .unwrap_or_else(|error| error.into_inner());
        let Some(snapshot) = snapshots.as_ref().filter(|snapshot| {
            snapshot.world_name == world_name
                && snapshot.world_generation.as_deref() == Some(world_generation)
                && matches!(
                    snapshot.state,
                    MatchState::Active | MatchState::ExtractionOpen
                )
        }) else {
            return false;
        };
        snapshot
            .participants
            .get(&account_id)
            .is_some_and(|participant| {
                participant.state == ParticipantState::Active
                    && participant.joined
                    && participant.public_player_id.to_string() == client_id
            })
    }
}
