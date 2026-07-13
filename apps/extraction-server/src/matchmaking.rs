use std::{collections::VecDeque, sync::Mutex};

use time::OffsetDateTime;
use uuid::Uuid;

mod command;
mod coordinator;
mod coordinator_connections;
#[cfg(any(feature = "engine", test))]
mod coordinator_deaths;
mod coordinator_events;
mod coordinator_lifecycle;
mod coordinator_participants;
mod coordinator_queue;
mod coordinator_timing;
mod gate;
#[cfg(feature = "engine")]
mod gate_access;
mod gate_reservations;
#[cfg(test)]
mod gate_tests;
mod gate_types;
mod model;
mod service;
#[cfg(any(feature = "engine", test))]
mod service_deaths;
#[cfg(test)]
mod service_tests;
mod service_ticker;
mod state;

pub use model::{
    ActivationDeadlines, CreatePreparingMatch, FrozenParticipant, FrozenRoster, FrozenRosterError,
    MatchDeathNotice, MatchRecord, MatchTimeoutNotice, MatchVersions, ParticipantDeath,
    ParticipantMatchStats, ParticipantRecord, ParticipantResourceCounts, ParticipantTimeout,
    QueuedPlayer, SeatId, SeatIdError, StoredMatch, MATCH_SIZE, RECONNECT_WINDOW,
};
pub use service::{
    MatchAttachKind, MatchConnectionEvent, MatchmakingError, MatchmakingService, QueueSnapshot,
    QueueStatus,
};
pub use state::{MatchState, ParticipantState, StateParseError, TransitionError};

#[derive(Debug, Default)]
pub struct MatchmakingQueue {
    entries: Mutex<VecDeque<QueueEntry>>,
}

impl MatchmakingQueue {
    pub fn enqueue(&self, account_id: Uuid, now: OffsetDateTime) -> QueuePosition {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(index) = entries
            .iter()
            .position(|entry| entry.account_id == account_id)
        {
            let entry = &entries[index];
            return QueuePosition {
                position: index + 1,
                enqueued_at: entry.enqueued_at,
            };
        }
        entries.push_back(QueueEntry {
            account_id,
            enqueued_at: now,
        });
        QueuePosition {
            position: entries.len(),
            enqueued_at: now,
        }
    }

    pub fn dequeue(&self, account_id: Uuid) -> bool {
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(index) = entries
            .iter()
            .position(|entry| entry.account_id == account_id)
        else {
            return false;
        };
        entries.remove(index);
        true
    }

    pub fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct QueuePosition {
    pub position: usize,
    pub enqueued_at: OffsetDateTime,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct QueueEntry {
    account_id: Uuid,
    enqueued_at: OffsetDateTime,
}
