use std::{collections::HashMap, time::Duration};

use uuid::Uuid;

use super::{MatchAttachKind, MatchState, ParticipantState};

pub(super) struct AttachRequest<'a> {
    pub world_name: &'a str,
    pub world_generation: &'a str,
    pub client_id: &'a str,
    pub attach_attempt_id: &'a str,
    pub account_id: Uuid,
    pub kind: MatchAttachKind,
}

#[derive(Clone)]
pub(super) struct GateSnapshot {
    pub world_name: String,
    pub world_generation: Option<String>,
    pub state: MatchState,
    pub participants: HashMap<Uuid, GateParticipant>,
}

#[derive(Clone, Copy)]
pub(super) struct GateParticipant {
    pub public_player_id: Uuid,
    pub state: ParticipantState,
    pub joined: bool,
    pub reconnect_deadline: Option<Duration>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum AttachDecision {
    Allowed,
    Denied,
    Expired,
}
