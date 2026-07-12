use std::{collections::VecDeque, sync::Mutex};

use time::OffsetDateTime;
use uuid::Uuid;

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
