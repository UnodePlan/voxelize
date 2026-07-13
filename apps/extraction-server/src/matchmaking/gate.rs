use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, RwLock,
    },
    time::Duration,
};

use time::OffsetDateTime;
use uuid::Uuid;

use super::{
    gate_reservations::{
        AttachIdentity, AttachReservations, JoinReservation, OwnedAttachIdentity, RebindReservation,
    },
    gate_types::{AttachDecision, AttachRequest, GateSnapshot},
    MatchAttachKind, MatchState, ParticipantState,
};

#[derive(Default)]
pub(super) struct AttachGate {
    failed_closed: AtomicBool,
    pub(super) snapshot: RwLock<Option<GateSnapshot>>,
    hard_deadline_closed: Mutex<Option<(String, String)>>,
    reservations: AttachReservations,
}

impl AttachGate {
    pub(super) fn replace(&self, mut snapshot: Option<GateSnapshot>) {
        let active_world = snapshot.as_ref().and_then(|snapshot| {
            snapshot
                .world_generation
                .as_ref()
                .map(|generation| (snapshot.world_name.clone(), generation.clone()))
        });
        let mut hard_deadline_closed = self
            .hard_deadline_closed
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match (hard_deadline_closed.as_ref(), active_world.as_ref()) {
            (Some(closed), Some(active)) if closed == active => {
                if let Some(snapshot) = snapshot.as_mut() {
                    snapshot.state = MatchState::Settling;
                }
            }
            (Some(_), _) => *hard_deadline_closed = None,
            (None, _) => {}
        }
        *self
            .snapshot
            .write()
            .unwrap_or_else(|error| error.into_inner()) = snapshot;
        self.reservations.retain_for(
            active_world
                .as_ref()
                .map(|(world_name, generation)| (world_name.as_str(), generation.as_str())),
        );
    }

    pub(super) fn fail_closed(&self) {
        self.failed_closed.store(true, Ordering::Release);
        self.reservations.clear();
    }

    pub(super) fn is_failed_closed(&self) -> bool {
        self.failed_closed.load(Ordering::Acquire)
    }

    pub(super) fn allows(
        &self,
        request: AttachRequest<'_>,
        now: Duration,
        utc_now: OffsetDateTime,
    ) -> AttachDecision {
        if self.is_failed_closed() {
            return AttachDecision::Denied;
        }
        let snapshots = self
            .snapshot
            .read()
            .unwrap_or_else(|error| error.into_inner());
        let Some(snapshot) = snapshots.as_ref().filter(|item| {
            item.world_name == request.world_name
                && item.world_generation.as_deref() == Some(request.world_generation)
        }) else {
            return AttachDecision::Denied;
        };
        let Some(participant) = snapshot.participants.get(&request.account_id) else {
            return AttachDecision::Denied;
        };
        if participant.public_player_id.to_string() != request.client_id {
            return AttachDecision::Denied;
        }
        let decision = match request.kind {
            MatchAttachKind::Join
                if snapshot.state == MatchState::Preparing
                    && participant.state == ParticipantState::Preparing
                    && !participant.joined =>
            {
                AttachDecision::Allowed
            }
            MatchAttachKind::Rebind
                if matches!(
                    snapshot.state,
                    MatchState::Active | MatchState::ExtractionOpen
                ) && participant.state == ParticipantState::Disconnected =>
            {
                match participant.reconnect_deadline {
                    Some(deadline) if now < deadline => AttachDecision::Allowed,
                    Some(_) => AttachDecision::Expired,
                    None => AttachDecision::Denied,
                }
            }
            _ => AttachDecision::Denied,
        };
        if request.kind == MatchAttachKind::Join && decision == AttachDecision::Allowed {
            self.reservations.reserve_join(
                request.account_id,
                JoinReservation {
                    identity: owned_identity(request),
                },
            );
        } else if request.kind == MatchAttachKind::Rebind
            && decision == AttachDecision::Allowed
            && !self.reservations.reserve_rebind(
                request.account_id,
                RebindReservation {
                    identity: owned_identity(request),
                    admitted_at: utc_now,
                },
            )
        {
            return AttachDecision::Denied;
        }
        drop(snapshots);
        decision
    }

    pub(super) fn close_for_hard_deadline(&self, world_name: &str, world_generation: &str) -> bool {
        let mut hard_deadline_closed = self
            .hard_deadline_closed
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut snapshot = self
            .snapshot
            .write()
            .unwrap_or_else(|error| error.into_inner());
        let Some(snapshot) = snapshot.as_mut().filter(|snapshot| {
            snapshot.world_name == world_name
                && snapshot.world_generation.as_deref() == Some(world_generation)
        }) else {
            return false;
        };
        if hard_deadline_closed
            .as_ref()
            .is_some_and(|closed| closed.0 == world_name && closed.1 == world_generation)
            && snapshot.state == MatchState::Settling
        {
            return true;
        }
        if !matches!(
            snapshot.state,
            MatchState::Active | MatchState::ExtractionOpen
        ) {
            return false;
        }
        *hard_deadline_closed = Some((world_name.to_owned(), world_generation.to_owned()));
        snapshot.state = MatchState::Settling;
        self.reservations.clear();
        true
    }

    pub(super) fn claim_rebind_timeout(
        &self,
        world_name: &str,
        world_generation: &str,
        account_id: Uuid,
    ) -> bool {
        self.reservations
            .claim_timeout(world_name, world_generation, account_id)
    }

    pub(super) fn release_rebind_timeout_claim(
        &self,
        world_name: &str,
        world_generation: &str,
        account_id: Uuid,
    ) {
        self.reservations
            .release_timeout_claim(world_name, world_generation, account_id);
    }

    pub(super) fn take_join_reservation(
        &self,
        world_name: &str,
        world_generation: &str,
        client_id: &str,
        attach_attempt_id: &str,
        account_id: Uuid,
    ) -> bool {
        self.reservations.take_join(
            AttachIdentity {
                world_name,
                world_generation,
                client_id,
                attach_attempt_id,
            },
            account_id,
        )
    }

    pub(super) fn take_rebind_reservation(
        &self,
        world_name: &str,
        world_generation: &str,
        client_id: &str,
        attach_attempt_id: &str,
        account_id: Uuid,
    ) -> Option<OffsetDateTime> {
        self.reservations.take_rebind(
            AttachIdentity {
                world_name,
                world_generation,
                client_id,
                attach_attempt_id,
            },
            account_id,
        )
    }

    pub(super) fn clear_attach_reservation(
        &self,
        world_name: &str,
        world_generation: &str,
        client_id: &str,
        attach_attempt_id: &str,
        account_id: Uuid,
    ) {
        self.take_join_reservation(
            world_name,
            world_generation,
            client_id,
            attach_attempt_id,
            account_id,
        );
        let _ = self.take_rebind_reservation(
            world_name,
            world_generation,
            client_id,
            attach_attempt_id,
            account_id,
        );
    }
}

fn owned_identity(request: AttachRequest<'_>) -> OwnedAttachIdentity {
    OwnedAttachIdentity {
        world_name: request.world_name.to_owned(),
        world_generation: request.world_generation.to_owned(),
        client_id: request.client_id.to_owned(),
        attach_attempt_id: request.attach_attempt_id.to_owned(),
    }
}
