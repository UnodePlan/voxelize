#[cfg(feature = "engine")]
use std::time::Duration;

#[cfg(feature = "engine")]
use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    gate_types::{AttachDecision, AttachRequest},
    MatchAttachKind, MatchmakingService,
};

impl MatchmakingService {
    pub fn allows_attach(
        &self,
        world_name: &str,
        world_generation: &str,
        client_id: &str,
        attach_attempt_id: &str,
        account_id: Uuid,
        kind: MatchAttachKind,
    ) -> bool {
        let now = self.clock.monotonic_now();
        let utc_now = self.clock.utc_now().into();
        let decision = self.gate.allows(
            AttachRequest {
                world_name,
                world_generation,
                client_id,
                attach_attempt_id,
                account_id,
                kind,
            },
            now,
            utc_now,
        );
        if decision == AttachDecision::Expired {
            self.schedule_tick_once();
        }
        decision == AttachDecision::Allowed
    }

    #[cfg(feature = "engine")]
    pub(crate) fn public_player_id_for(
        &self,
        world_name: &str,
        account_id: Uuid,
    ) -> Option<String> {
        self.gate.public_player_id(world_name, account_id)
    }

    #[cfg(feature = "engine")]
    pub(crate) fn allows_gameplay(
        &self,
        world_name: &str,
        world_generation: &str,
        client_id: &str,
        account_id: Uuid,
    ) -> bool {
        self.gate
            .allows_gameplay(world_name, world_generation, client_id, account_id)
    }

    #[cfg(feature = "engine")]
    pub(crate) fn monotonic_now(&self) -> Duration {
        self.clock.monotonic_now()
    }

    #[cfg(feature = "engine")]
    pub(crate) fn utc_now(&self) -> OffsetDateTime {
        self.clock.utc_now().into()
    }

    #[cfg(feature = "engine")]
    pub(crate) fn gameplay_timeline(
        &self,
        world_name: &str,
        world_generation: &str,
    ) -> Option<super::GameplayTimeline> {
        self.gate.gameplay_timeline(world_name, world_generation)
    }
}
