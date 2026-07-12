use std::fmt;

use serde::Serialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum WorldLifecycleState {
    Created,
    Preparing,
    Ready,
    Stopping,
    Stopped,
}

impl WorldLifecycleState {
    pub fn accepts_clients(self) -> bool {
        self == Self::Ready
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientJoinError {
    WorldNotReady(WorldLifecycleState),
    WorldFull { capacity: usize },
    DuplicateClient,
    DuplicatePrincipal,
    AdmissionDenied,
    JoinCancelled,
}

impl fmt::Display for ClientJoinError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorldNotReady(state) => write!(formatter, "world is not ready: {state:?}"),
            Self::WorldFull { capacity } => write!(formatter, "world is full ({capacity})"),
            Self::DuplicateClient => formatter.write_str("client is already in this world"),
            Self::DuplicatePrincipal => formatter.write_str("principal is already in this world"),
            Self::AdmissionDenied => formatter.write_str("client admission was denied"),
            Self::JoinCancelled => formatter.write_str("join attempt was cancelled"),
        }
    }
}

impl std::error::Error for ClientJoinError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientJoinReceipt {
    pub client_id: String,
    pub join_attempt_id: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClientDetachOutcome {
    Detached,
    Despawned,
    NotFound,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClientRebindError {
    NotFound,
    PrincipalMismatch,
    ClientAlreadyAttached,
    WorldNotReady(WorldLifecycleState),
    AdmissionDenied,
}

impl fmt::Display for ClientRebindError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => formatter.write_str("detached client was not found"),
            Self::PrincipalMismatch => {
                formatter.write_str("principal does not own detached client")
            }
            Self::ClientAlreadyAttached => formatter.write_str("client is already attached"),
            Self::WorldNotReady(state) => write!(formatter, "world is not ready: {state:?}"),
            Self::AdmissionDenied => formatter.write_str("client admission was denied"),
        }
    }
}

impl std::error::Error for ClientRebindError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorldStopSummary {
    pub client_ids: Vec<String>,
}
