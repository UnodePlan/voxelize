use std::{
    collections::{BTreeMap, HashMap, HashSet, VecDeque},
    sync::Arc,
    time::Duration,
};

use time::OffsetDateTime;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::{
    command::Command,
    gate::AttachGate,
    gate_types::{GateParticipant, GateSnapshot},
    service::{MatchmakingError, QueueSnapshot, QueueStatus},
    CreatePreparingMatch, MatchState, MatchVersions, ParticipantState,
};
use crate::observability::{MatchEvent, MatchEventSink, StderrMatchEventSink};
use crate::ports::{
    Clock, IdGenerator, MatchWorldRuntime, MatchWorldRuntimeError, MatchmakingRepository,
    SeedGenerator, SettlingTrigger,
};

pub(super) struct Coordinator {
    pub(super) repository: Arc<dyn MatchmakingRepository>,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) ids: Arc<dyn IdGenerator>,
    pub(super) seeds: Arc<dyn SeedGenerator>,
    pub(super) versions: MatchVersions,
    /// 本进程成局人数（生产 10；DEV 可为 2..=10）。
    pub(super) match_size: usize,
    pub(super) events: Arc<dyn MatchEventSink>,
    pub(super) gate: Arc<AttachGate>,
    pub(super) sender: mpsc::Sender<Command>,
    pub(super) runtime: Option<Arc<dyn MatchWorldRuntime>>,
    pub(super) queue: VecDeque<QueueEntry>,
    pub(super) connections: HashMap<Uuid, HashSet<String>>,
    pub(super) current: Option<LiveMatch>,
    pub(super) prepare_attempt: Option<CreatePreparingMatch>,
    pub(super) pending_settlements: BTreeMap<(Uuid, Uuid), PendingSettlement>,
    pub(super) next_order: u64,
}

impl Coordinator {
    pub(super) fn new(
        repository: Arc<dyn MatchmakingRepository>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGenerator>,
        seeds: Arc<dyn SeedGenerator>,
        versions: MatchVersions,
        gate: Arc<AttachGate>,
        sender: mpsc::Sender<Command>,
        match_size: usize,
    ) -> Self {
        let match_size =
            super::sanitize_match_capacity(match_size).unwrap_or(super::MATCH_SIZE);
        Self {
            repository,
            clock,
            ids,
            seeds,
            versions,
            match_size,
            events: Arc::new(StderrMatchEventSink),
            gate,
            sender,
            runtime: None,
            queue: VecDeque::new(),
            connections: HashMap::new(),
            current: None,
            prepare_attempt: None,
            pending_settlements: BTreeMap::new(),
            next_order: 0,
        }
    }

    pub(super) fn with_event_sink(mut self, events: Arc<dyn MatchEventSink>) -> Self {
        self.events = events;
        self
    }

    pub(super) fn utc_now(&self) -> OffsetDateTime {
        self.clock.utc_now().into()
    }

    pub(super) fn monotonic_deadline_for(
        &self,
        utc_deadline: OffsetDateTime,
    ) -> Result<Duration, MatchmakingError> {
        let remaining_nanos = (utc_deadline - self.utc_now()).whole_nanoseconds().max(0);
        let remaining_nanos =
            u64::try_from(remaining_nanos).map_err(|_| MatchmakingError::Unavailable)?;
        self.clock
            .monotonic_now()
            .checked_add(Duration::from_nanos(remaining_nanos))
            .ok_or(MatchmakingError::Unavailable)
    }

    pub(super) fn is_connected(&self, account_id: Uuid) -> bool {
        self.connections
            .get(&account_id)
            .is_some_and(|connections| !connections.is_empty())
    }

    pub(super) fn sync_gate(&self) {
        self.gate
            .replace(self.current.as_ref().map(LiveMatch::gate_snapshot));
    }

    pub(super) fn record_event(&self, event: MatchEvent) {
        self.events.record(event);
    }

    pub(super) fn current_match_id(&self) -> Option<Uuid> {
        self.current.as_ref().map(|current| current.match_id)
    }

    pub(super) fn snapshot_for(&self, account_id: Uuid) -> Option<QueueSnapshot> {
        if let Some(index) = self
            .queue
            .iter()
            .position(|entry| entry.account_id == account_id)
        {
            let entry = &self.queue[index];
            return Some(QueueSnapshot {
                status: QueueStatus::Queued,
                position: Some(index + 1),
                enqueued_at: Some(entry.enqueued_at),
                match_id: None,
                world_name: None,
                removed: None,
            });
        }
        if let Some(current) = self.current.as_ref().filter(|item| {
            item.participants
                .get(&account_id)
                .is_some_and(|participant| !participant.state.is_terminal())
        }) {
            return Some(current.snapshot());
        }
        None
    }

    pub(super) fn restore_waiting(&mut self, entries: Vec<QueueEntry>) {
        self.prepare_attempt = None;
        let mut combined = self.queue.drain(..).collect::<Vec<_>>();
        combined.extend(
            entries
                .into_iter()
                .filter(|entry| self.is_connected(entry.account_id)),
        );
        combined.sort_by_key(|entry| (entry.enqueued_at, entry.order));
        let mut seen_accounts = HashSet::new();
        combined.retain(|entry| seen_accounts.insert(entry.account_id));
        self.queue = combined.into();
    }
}

#[derive(Clone, Debug)]
pub(super) struct QueueEntry {
    pub account_id: Uuid,
    pub enqueued_at: OffsetDateTime,
    pub order: u64,
}

pub(super) struct LiveMatch {
    pub match_id: Uuid,
    pub world_name: String,
    pub world_generation: Option<String>,
    pub state: MatchState,
    pub original_queue: Vec<QueueEntry>,
    pub participants: HashMap<Uuid, LiveParticipant>,
    pub extraction_open_deadline: Option<Duration>,
    pub hard_deadline: Option<Duration>,
    pub extraction_open_at_utc: Option<OffsetDateTime>,
    pub hard_deadline_utc: Option<OffsetDateTime>,
    pub settlement_grace_deadline_utc: Option<OffsetDateTime>,
    pub activated_at: Option<Duration>,
    pub abort_reason: Option<String>,
    pub settling_trigger: Option<SettlingTrigger>,
    pub settling_persisted: bool,
    pub world_stopped: bool,
    pub hard_deadline_task: Option<tokio::task::JoinHandle<()>>,
    pub hard_deadline_closing: bool,
}

#[derive(Clone, Debug)]
pub(super) struct PendingSettlement {
    pub command: crate::matchmaking::CommitSettlement,
    pub grace_deadline: OffsetDateTime,
    pub pending_persisted: bool,
    pub read_before_write: bool,
    pub post_grace_reads_remaining: u8,
}

impl LiveMatch {
    pub(super) fn snapshot(&self) -> QueueSnapshot {
        let status = match self.state {
            MatchState::Waiting => QueueStatus::Queued,
            MatchState::Preparing => QueueStatus::Preparing,
            MatchState::Active => QueueStatus::Active,
            MatchState::ExtractionOpen => QueueStatus::ExtractionOpen,
            MatchState::Settling | MatchState::Finished | MatchState::Aborted => {
                QueueStatus::Settling
            }
        };
        QueueSnapshot {
            status,
            position: None,
            enqueued_at: None,
            match_id: Some(self.match_id),
            world_name: Some(self.world_name.clone()),
            removed: None,
        }
    }

    fn gate_snapshot(&self) -> GateSnapshot {
        GateSnapshot {
            world_name: self.world_name.clone(),
            world_generation: self.world_generation.clone(),
            state: self.state,
            #[cfg(feature = "engine")]
            extraction_open: self.state == MatchState::ExtractionOpen,
            #[cfg(feature = "engine")]
            hard_deadline: self.hard_deadline,
            #[cfg(feature = "engine")]
            extraction_open_at_utc: self.extraction_open_at_utc,
            #[cfg(feature = "engine")]
            hard_deadline_utc: self.hard_deadline_utc,
            participants: self
                .participants
                .iter()
                .map(|(account_id, participant)| {
                    (
                        *account_id,
                        GateParticipant {
                            public_player_id: participant.public_player_id,
                            state: participant.state,
                            joined: participant.joined,
                            reconnect_deadline: participant.reconnect_deadline,
                        },
                    )
                })
                .collect(),
        }
    }
}

pub(super) struct LiveParticipant {
    pub public_player_id: Uuid,
    pub state: ParticipantState,
    pub joined: bool,
    pub control_connection: Option<String>,
    pub reconnect_deadline: Option<Duration>,
    pub despawn_pending: bool,
}

pub(super) fn repository_error(error: crate::ports::MatchRepositoryError) -> MatchmakingError {
    match error {
        crate::ports::MatchRepositoryError::SeatOccupied
        | crate::ports::MatchRepositoryError::Conflict => MatchmakingError::RosterLocked,
        crate::ports::MatchRepositoryError::Unavailable => MatchmakingError::Unavailable,
    }
}

pub(super) fn runtime_error(error: MatchWorldRuntimeError) -> MatchmakingError {
    match error {
        MatchWorldRuntimeError::Conflict => MatchmakingError::RosterLocked,
        MatchWorldRuntimeError::Unavailable => MatchmakingError::Unavailable,
    }
}
