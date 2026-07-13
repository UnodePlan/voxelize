use std::{collections::HashMap, time::Duration};

#[cfg(feature = "engine")]
use time::OffsetDateTime;
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
    #[cfg(feature = "engine")]
    pub extraction_open: bool,
    #[cfg(feature = "engine")]
    pub hard_deadline: Option<Duration>,
    #[cfg(feature = "engine")]
    pub extraction_open_at_utc: Option<OffsetDateTime>,
    #[cfg(feature = "engine")]
    pub hard_deadline_utc: Option<OffsetDateTime>,
    pub participants: HashMap<Uuid, GateParticipant>,
}

#[cfg(feature = "engine")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct GameplayTimeline {
    pub extraction_open: bool,
    pub hard_deadline: Duration,
    pub extraction_open_at_utc: OffsetDateTime,
    pub hard_deadline_utc: OffsetDateTime,
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
