use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};

use time::OffsetDateTime;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use super::{
    command::Command,
    coordinator::Coordinator,
    gate::AttachGate,
    gate_types::{AttachDecision, AttachRequest},
    MatchVersions,
};
use crate::ports::{Clock, IdGenerator, MatchRepository, MatchWorldRuntime, SeedGenerator};

const COMMAND_CAPACITY: usize = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum QueueStatus {
    Idle,
    Queued,
    Preparing,
    Active,
    ExtractionOpen,
    Settling,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct QueueSnapshot {
    pub status: QueueStatus,
    pub position: Option<usize>,
    pub enqueued_at: Option<OffsetDateTime>,
    pub match_id: Option<Uuid>,
    pub world_name: Option<String>,
    pub removed: Option<bool>,
}

impl QueueSnapshot {
    pub(super) fn idle(removed: bool) -> Self {
        Self {
            status: QueueStatus::Idle,
            position: None,
            enqueued_at: None,
            match_id: None,
            world_name: None,
            removed: Some(removed),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchmakingError {
    ConnectionRequired,
    Full,
    RosterLocked,
    ReconnectExpired,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MatchConnectionEvent {
    Connected {
        connection_id: String,
        account_id: Uuid,
    },
    JoinCommitted {
        connection_id: String,
        account_id: Uuid,
        world_name: String,
        world_generation: String,
        client_id: String,
        attach_attempt_id: String,
    },
    Disconnected {
        connection_id: String,
        account_id: Uuid,
        observed_at: Duration,
        world_name: Option<String>,
        world_generation: Option<String>,
        client_id: Option<String>,
        attach_attempt_id: Option<String>,
    },
    Detached {
        account_id: Uuid,
        world_name: String,
        world_generation: String,
        client_id: String,
        attach_attempt_id: String,
    },
    Rebound {
        connection_id: String,
        account_id: Uuid,
        world_name: String,
        world_generation: String,
        client_id: String,
        attach_attempt_id: String,
    },
    RebindRejected {
        account_id: Uuid,
        world_name: String,
        world_generation: String,
        client_id: String,
        attach_attempt_id: String,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchAttachKind {
    Join,
    Rebind,
}

pub struct MatchmakingService {
    pub(super) sender: mpsc::Sender<Command>,
    gate: Arc<AttachGate>,
    clock: Arc<dyn Clock>,
    pub(super) tick_pending: Arc<AtomicBool>,
    overflow_recovery_started: AtomicBool,
}

impl MatchmakingService {
    pub fn start(
        repository: Arc<dyn MatchRepository>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGenerator>,
        seeds: Arc<dyn SeedGenerator>,
        versions: MatchVersions,
    ) -> Arc<Self> {
        let (sender, receiver) = mpsc::channel(COMMAND_CAPACITY);
        let gate = Arc::new(AttachGate::default());
        let tick_pending = Arc::new(AtomicBool::new(false));
        let coordinator = Coordinator::new(
            repository,
            clock.clone(),
            ids,
            seeds,
            versions,
            gate.clone(),
        );
        tokio::spawn(coordinator.run(receiver));
        Arc::new(Self {
            sender,
            gate,
            clock,
            tick_pending,
            overflow_recovery_started: AtomicBool::new(false),
        })
    }

    pub async fn bind_runtime(
        &self,
        runtime: Arc<dyn MatchWorldRuntime>,
    ) -> Result<(), MatchmakingError> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Command::BindRuntime { runtime, reply })
            .await
            .map_err(|_| MatchmakingError::Unavailable)?;
        response.await.map_err(|_| MatchmakingError::Unavailable)
    }

    pub async fn enqueue(&self, account_id: Uuid) -> Result<QueueSnapshot, MatchmakingError> {
        self.request(|reply| Command::Enqueue { account_id, reply })
            .await
    }

    pub async fn cancel(&self, account_id: Uuid) -> Result<QueueSnapshot, MatchmakingError> {
        self.request(|reply| Command::Cancel { account_id, reply })
            .await
    }

    pub async fn apply_connection_event(
        &self,
        event: MatchConnectionEvent,
    ) -> Result<(), MatchmakingError> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Command::Connection {
                event,
                reply: Some(reply),
            })
            .await
            .map_err(|_| MatchmakingError::Unavailable)?;
        response.await.map_err(|_| MatchmakingError::Unavailable)?
    }

    pub fn observe_connection_event(&self, event: MatchConnectionEvent) {
        if self
            .sender
            .try_send(Command::Connection { event, reply: None })
            .is_err()
        {
            self.fail_closed_after_overflow();
        }
    }

    #[cfg(feature = "engine")]
    pub(crate) fn fail_closed(&self) {
        self.fail_closed_after_overflow();
    }

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

    pub async fn tick(&self) -> Result<(), MatchmakingError> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Command::Tick {
                reply: Some(reply),
                ticker_pending: None,
            })
            .await
            .map_err(|_| MatchmakingError::Unavailable)?;
        response.await.map_err(|_| MatchmakingError::Unavailable)?
    }

    async fn request<T>(
        &self,
        command: impl FnOnce(oneshot::Sender<Result<T, MatchmakingError>>) -> Command,
    ) -> Result<T, MatchmakingError> {
        if self.gate.is_failed_closed() {
            return Err(MatchmakingError::Unavailable);
        }
        let (reply, response) = oneshot::channel();
        self.sender
            .send(command(reply))
            .await
            .map_err(|_| MatchmakingError::Unavailable)?;
        response.await.map_err(|_| MatchmakingError::Unavailable)?
    }

    fn fail_closed_after_overflow(&self) {
        self.gate.fail_closed();
        if self.overflow_recovery_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let sender = self.sender.clone();
        tokio::spawn(async move {
            let _ = sender.send(Command::FailClosed).await;
        });
    }
}
