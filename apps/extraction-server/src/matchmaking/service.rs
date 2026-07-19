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

#[cfg(any(test, feature = "e2e-control"))]
use super::coordinator_diagnostics::CoordinatorResourceSnapshot;
use super::{command::Command, coordinator::Coordinator, gate::AttachGate, MatchVersions};
use crate::observability::{MatchEvent, MatchEventSink, RejectionReason, StderrMatchEventSink};
use crate::ports::{Clock, IdGenerator, MatchWorldRuntime, MatchmakingRepository, SeedGenerator};

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
    pub(super) gate: Arc<AttachGate>,
    pub(super) clock: Arc<dyn Clock>,
    runtime: tokio::runtime::Handle,
    pub(super) tick_pending: Arc<AtomicBool>,
    pub(super) events: Arc<dyn MatchEventSink>,
    overflow_recovery_started: AtomicBool,
}

impl MatchmakingService {
    pub fn start(
        repository: Arc<dyn MatchmakingRepository>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGenerator>,
        seeds: Arc<dyn SeedGenerator>,
        versions: MatchVersions,
    ) -> Arc<Self> {
        Self::start_with_match_size(
            repository,
            clock,
            ids,
            seeds,
            versions,
            super::MATCH_SIZE,
        )
    }

    /// 指定成局人数（2..=MATCH_SIZE）；测试与 DEV 联调用。
    pub fn start_with_match_size(
        repository: Arc<dyn MatchmakingRepository>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGenerator>,
        seeds: Arc<dyn SeedGenerator>,
        versions: MatchVersions,
        match_size: usize,
    ) -> Arc<Self> {
        Self::start_with_capacity(
            repository,
            clock,
            ids,
            seeds,
            versions,
            match_size,
            Arc::new(StderrMatchEventSink),
        )
    }

    #[cfg(any(test, feature = "e2e-control"))]
    pub(crate) fn start_with_event_sink(
        repository: Arc<dyn MatchmakingRepository>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGenerator>,
        seeds: Arc<dyn SeedGenerator>,
        versions: MatchVersions,
        events: Arc<dyn MatchEventSink>,
    ) -> Arc<Self> {
        Self::start_with_event_sink_and_size(
            repository,
            clock,
            ids,
            seeds,
            versions,
            super::MATCH_SIZE,
            events,
        )
    }

    #[cfg(any(test, feature = "e2e-control"))]
    pub(crate) fn start_with_event_sink_and_size(
        repository: Arc<dyn MatchmakingRepository>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGenerator>,
        seeds: Arc<dyn SeedGenerator>,
        versions: MatchVersions,
        match_size: usize,
        events: Arc<dyn MatchEventSink>,
    ) -> Arc<Self> {
        Self::start_with_capacity(
            repository,
            clock,
            ids,
            seeds,
            versions,
            match_size,
            events,
        )
    }

    fn start_with_capacity(
        repository: Arc<dyn MatchmakingRepository>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGenerator>,
        seeds: Arc<dyn SeedGenerator>,
        versions: MatchVersions,
        match_size: usize,
        events: Arc<dyn MatchEventSink>,
    ) -> Arc<Self> {
        let (sender, receiver) = mpsc::channel(COMMAND_CAPACITY);
        let gate = Arc::new(AttachGate::default());
        let tick_pending = Arc::new(AtomicBool::new(false));
        let runtime = tokio::runtime::Handle::current();
        let coordinator = Coordinator::new(
            repository,
            clock.clone(),
            ids,
            seeds,
            versions,
            gate.clone(),
            sender.clone(),
            match_size,
        )
        .with_event_sink(events.clone());
        runtime.spawn(coordinator.run(receiver));
        Arc::new(Self {
            sender,
            gate,
            clock,
            runtime,
            tick_pending,
            events,
            overflow_recovery_started: AtomicBool::new(false),
        })
    }

    #[cfg(any(test, feature = "e2e-control"))]
    pub(crate) async fn resource_snapshot(
        &self,
    ) -> Result<CoordinatorResourceSnapshot, MatchmakingError> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Command::InspectResources { reply })
            .await
            .map_err(|_| MatchmakingError::Unavailable)?;
        let mut snapshot = response.await.map_err(|_| MatchmakingError::Unavailable)?;
        snapshot.ticker_pending = self.tick_pending.load(Ordering::Acquire);
        Ok(snapshot)
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

    pub async fn queue_snapshot(
        &self,
        account_id: Uuid,
    ) -> Result<QueueSnapshot, MatchmakingError> {
        self.query(|reply| Command::FindQueueSnapshot { account_id, reply })
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
            self.events.record(MatchEvent::RequestRejected {
                match_id: None,
                reason: RejectionReason::Unavailable,
            });
            return Err(MatchmakingError::Unavailable);
        }
        self.query(command).await
    }

    pub(super) async fn query<T>(
        &self,
        command: impl FnOnce(oneshot::Sender<Result<T, MatchmakingError>>) -> Command,
    ) -> Result<T, MatchmakingError> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(command(reply))
            .await
            .map_err(|_| MatchmakingError::Unavailable)?;
        response.await.map_err(|_| MatchmakingError::Unavailable)?
    }

    pub(super) fn fail_closed_after_overflow(&self) {
        self.gate.fail_closed();
        if self.overflow_recovery_started.swap(true, Ordering::AcqRel) {
            return;
        }
        let sender = self.sender.clone();
        self.runtime.spawn(async move {
            let _ = sender.send(Command::FailClosed).await;
        });
    }
}
