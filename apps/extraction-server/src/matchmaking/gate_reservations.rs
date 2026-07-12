use std::{collections::HashMap, sync::Mutex};

use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Default)]
pub(super) struct AttachReservations {
    joins: Mutex<HashMap<Uuid, JoinReservation>>,
    rebinds: Mutex<RebindReservations>,
}

impl AttachReservations {
    pub(super) fn retain_for(&self, active_world: Option<(&str, &str)>) {
        self.joins
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|_, reservation| reservation.belongs_to(active_world));
        self.rebinds
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain_for(active_world);
    }

    pub(super) fn clear(&self) {
        self.joins
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
        self.rebinds
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear_all();
    }

    pub(super) fn reserve_join(&self, account_id: Uuid, reservation: JoinReservation) {
        self.joins
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(account_id, reservation);
    }

    pub(super) fn reserve_rebind(&self, account_id: Uuid, reservation: RebindReservation) -> bool {
        let mut rebinds = self
            .rebinds
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if rebinds
            .timeout_claims
            .get(&account_id)
            .is_some_and(|claim| claim.same_world(&reservation.identity))
        {
            return false;
        }
        rebinds.reservations.insert(account_id, reservation);
        true
    }

    pub(super) fn claim_timeout(
        &self,
        world_name: &str,
        world_generation: &str,
        account_id: Uuid,
    ) -> bool {
        let mut rebinds = self
            .rebinds
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if rebinds
            .reservations
            .get(&account_id)
            .is_some_and(|reservation| {
                reservation.identity.world_name == world_name
                    && reservation.identity.world_generation == world_generation
            })
        {
            return false;
        }
        if rebinds
            .timeout_claims
            .get(&account_id)
            .is_some_and(|claim| claim.matches(world_name, world_generation))
        {
            return false;
        }
        rebinds.timeout_claims.insert(
            account_id,
            TimeoutClaim {
                world_name: world_name.to_owned(),
                world_generation: world_generation.to_owned(),
            },
        );
        true
    }

    pub(super) fn release_timeout_claim(
        &self,
        world_name: &str,
        world_generation: &str,
        account_id: Uuid,
    ) {
        let mut rebinds = self
            .rebinds
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let matches = rebinds
            .timeout_claims
            .get(&account_id)
            .is_some_and(|claim| claim.matches(world_name, world_generation));
        if matches {
            rebinds.timeout_claims.remove(&account_id);
        }
    }

    pub(super) fn take_join(&self, identity: AttachIdentity<'_>, account_id: Uuid) -> bool {
        let mut joins = self.joins.lock().unwrap_or_else(|error| error.into_inner());
        let matches = joins
            .get(&account_id)
            .is_some_and(|reservation| reservation.identity.matches(identity));
        if matches {
            joins.remove(&account_id);
        }
        matches
    }

    pub(super) fn take_rebind(
        &self,
        identity: AttachIdentity<'_>,
        account_id: Uuid,
    ) -> Option<OffsetDateTime> {
        let mut state = self
            .rebinds
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let matches = state
            .reservations
            .get(&account_id)
            .is_some_and(|reservation| reservation.identity.matches(identity));
        matches.then(|| state.reservations.remove(&account_id).unwrap().admitted_at)
    }
}

#[derive(Default)]
struct RebindReservations {
    reservations: HashMap<Uuid, RebindReservation>,
    timeout_claims: HashMap<Uuid, TimeoutClaim>,
}

impl RebindReservations {
    fn retain_for(&mut self, active_world: Option<(&str, &str)>) {
        self.reservations
            .retain(|_, reservation| reservation.belongs_to(active_world));
        self.timeout_claims
            .retain(|_, claim| claim.belongs_to(active_world));
    }

    fn clear_all(&mut self) {
        self.reservations.clear();
        self.timeout_claims.clear();
    }
}

struct TimeoutClaim {
    world_name: String,
    world_generation: String,
}

impl TimeoutClaim {
    fn belongs_to(&self, active_world: Option<(&str, &str)>) -> bool {
        active_world.is_some_and(|(world_name, generation)| self.matches(world_name, generation))
    }

    fn matches(&self, world_name: &str, world_generation: &str) -> bool {
        self.world_name == world_name && self.world_generation == world_generation
    }

    fn same_world(&self, identity: &OwnedAttachIdentity) -> bool {
        self.matches(&identity.world_name, &identity.world_generation)
    }
}

pub(super) struct JoinReservation {
    pub(super) identity: OwnedAttachIdentity,
}

pub(super) struct RebindReservation {
    pub(super) identity: OwnedAttachIdentity,
    pub(super) admitted_at: OffsetDateTime,
}

pub(super) struct OwnedAttachIdentity {
    pub(super) world_name: String,
    pub(super) world_generation: String,
    pub(super) client_id: String,
    pub(super) attach_attempt_id: String,
}

impl OwnedAttachIdentity {
    fn matches(&self, other: AttachIdentity<'_>) -> bool {
        self.world_name == other.world_name
            && self.world_generation == other.world_generation
            && self.client_id == other.client_id
            && self.attach_attempt_id == other.attach_attempt_id
    }
}

impl JoinReservation {
    fn belongs_to(&self, active_world: Option<(&str, &str)>) -> bool {
        belongs_to(&self.identity, active_world)
    }
}

impl RebindReservation {
    fn belongs_to(&self, active_world: Option<(&str, &str)>) -> bool {
        belongs_to(&self.identity, active_world)
    }
}

#[derive(Clone, Copy)]
pub(super) struct AttachIdentity<'a> {
    pub(super) world_name: &'a str,
    pub(super) world_generation: &'a str,
    pub(super) client_id: &'a str,
    pub(super) attach_attempt_id: &'a str,
}

fn belongs_to(identity: &OwnedAttachIdentity, active_world: Option<(&str, &str)>) -> bool {
    active_world.is_some_and(|(world_name, generation)| {
        identity.world_name == world_name && identity.world_generation == generation
    })
}
