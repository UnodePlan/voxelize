use super::{coordinator::Coordinator, MatchConnectionEvent, MatchmakingError};

impl Coordinator {
    pub(super) async fn apply_connection(
        &mut self,
        event: MatchConnectionEvent,
    ) -> Result<(), MatchmakingError> {
        match event {
            MatchConnectionEvent::Connected {
                connection_id,
                account_id,
            } => {
                self.add_connection(account_id, connection_id);
                Ok(())
            }
            MatchConnectionEvent::JoinCommitted {
                connection_id,
                account_id,
                world_name,
                world_generation,
                client_id,
                attach_attempt_id,
            } => {
                self.add_connection(account_id, connection_id.clone());
                self.join_committed(
                    account_id,
                    connection_id,
                    &world_name,
                    &world_generation,
                    &client_id,
                    &attach_attempt_id,
                )
                .await
            }
            MatchConnectionEvent::Disconnected {
                connection_id,
                account_id,
                observed_at,
                world_name,
                world_generation,
                client_id,
                attach_attempt_id,
            } => {
                self.disconnected(
                    account_id,
                    &connection_id,
                    observed_at,
                    world_name.as_deref(),
                    world_generation.as_deref(),
                    client_id.as_deref(),
                    attach_attempt_id.as_deref(),
                )
                .await
            }
            MatchConnectionEvent::Detached {
                account_id,
                world_name,
                world_generation,
                client_id,
                attach_attempt_id,
            } => {
                self.gate.clear_attach_reservation(
                    &world_name,
                    &world_generation,
                    &client_id,
                    &attach_attempt_id,
                    account_id,
                );
                Ok(())
            }
            MatchConnectionEvent::Rebound {
                connection_id,
                account_id,
                world_name,
                world_generation,
                client_id,
                attach_attempt_id,
            } => {
                self.add_connection(account_id, connection_id.clone());
                self.rebound(
                    account_id,
                    connection_id,
                    &world_name,
                    &world_generation,
                    &client_id,
                    &attach_attempt_id,
                )
                .await
            }
            MatchConnectionEvent::RebindRejected {
                account_id,
                world_name,
                world_generation,
                client_id,
                attach_attempt_id,
            } => {
                self.gate.clear_attach_reservation(
                    &world_name,
                    &world_generation,
                    &client_id,
                    &attach_attempt_id,
                    account_id,
                );
                Ok(())
            }
        }
    }
}
