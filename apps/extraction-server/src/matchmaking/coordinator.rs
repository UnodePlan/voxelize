use std::{
    collections::{HashMap, HashSet, VecDeque},
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
use crate::ports::{
    Clock, IdGenerator, MatchRepository, MatchWorldRuntime, MatchWorldRuntimeError, SeedGenerator,
    SettlingTrigger,
};

pub(super) struct Coordinator {
    pub(super) repository: Arc<dyn MatchRepository>,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) ids: Arc<dyn IdGenerator>,
    pub(super) seeds: Arc<dyn SeedGenerator>,
    pub(super) versions: MatchVersions,
    pub(super) gate: Arc<AttachGate>,
    pub(super) runtime: Option<Arc<dyn MatchWorldRuntime>>,
    pub(super) queue: VecDeque<QueueEntry>,
    pub(super) connections: HashMap<Uuid, HashSet<String>>,
    pub(super) current: Option<LiveMatch>,
    pub(super) prepare_attempt: Option<CreatePreparingMatch>,
    pub(super) next_order: u64,
}

impl Coordinator {
    pub(super) fn new(
        repository: Arc<dyn MatchRepository>,
        clock: Arc<dyn Clock>,
        ids: Arc<dyn IdGenerator>,
        seeds: Arc<dyn SeedGenerator>,
        versions: MatchVersions,
        gate: Arc<AttachGate>,
    ) -> Self {
        Self {
            repository,
            clock,
            ids,
            seeds,
            versions,
            gate,
            runtime: None,
            queue: VecDeque::new(),
            connections: HashMap::new(),
            current: None,
            prepare_attempt: None,
            next_order: 0,
        }
    }

    pub(super) async fn run(mut self, mut receiver: mpsc::Receiver<Command>) {
        while let Some(command) = receiver.recv().await {
            match command {
                Command::BindRuntime { runtime, reply } => {
                    if self.runtime.is_none() {
                        self.runtime = Some(runtime);
                    }
                    let _ = reply.send(());
                }
                Command::Enqueue { account_id, reply } => {
                    let result = self.enqueue(account_id).await;
                    let _ = reply.send(result);
                }
                Command::Cancel { account_id, reply } => {
                    let result = self.cancel(account_id).await;
                    let _ = reply.send(result);
                }
                Command::Connection { event, reply } => {
                    let observed = reply.is_none();
                    let result = self.apply_connection(event).await;
                    if let Some(reply) = reply {
                        let _ = reply.send(result);
                    } else if observed && result.is_err() {
                        self.gate.fail_closed();
                        let _ = self.abort_current("connection_event_failed").await;
                    }
                }
                Command::Tick {
                    reply,
                    ticker_pending,
                } => {
                    if let Some(pending) = ticker_pending {
                        pending.store(false, std::sync::atomic::Ordering::Release);
                    }
                    let result = self.advance_time().await;
                    if let Some(reply) = reply {
                        let _ = reply.send(result);
                    }
                }
                Command::FailClosed => {
                    let _ = self.abort_current("connection_event_overflow").await;
                }
            }
        }
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

    pub(super) fn snapshot_for(&self, account_id: Uuid) -> Option<QueueSnapshot> {
        if let Some(current) = self
            .current
            .as_ref()
            .filter(|item| item.participants.contains_key(&account_id))
        {
            return Some(current.snapshot());
        }
        self.queue
            .iter()
            .position(|entry| entry.account_id == account_id)
            .map(|index| {
                let entry = &self.queue[index];
                QueueSnapshot {
                    status: QueueStatus::Queued,
                    position: Some(index + 1),
                    enqueued_at: Some(entry.enqueued_at),
                    match_id: None,
                    world_name: None,
                    removed: None,
                }
            })
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
    pub activated_at: Option<Duration>,
    pub abort_reason: Option<String>,
    pub settling_trigger: Option<SettlingTrigger>,
    pub settling_persisted: bool,
    pub world_stopped: bool,
    pub hard_deadline_task: Option<tokio::task::JoinHandle<()>>,
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
