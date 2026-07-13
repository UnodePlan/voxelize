use std::collections::VecDeque;

use specs::Entity;
use uuid::Uuid;

use crate::contracts::DropSlotPayload;

#[derive(Debug)]
pub(super) struct QueuedDropSlotIntent {
    pub entity: Entity,
    pub client_id: String,
    pub request_id: Uuid,
    pub sequence: u32,
    pub payload: DropSlotPayload,
}

pub(super) struct DropSlotIntentQueue {
    capacity: usize,
    intents: VecDeque<QueuedDropSlotIntent>,
}

impl DropSlotIntentQueue {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            intents: VecDeque::with_capacity(capacity),
        }
    }

    pub(super) fn push(
        &mut self,
        intent: QueuedDropSlotIntent,
    ) -> Result<(), QueuedDropSlotIntent> {
        if self.intents.len() >= self.capacity {
            return Err(intent);
        }
        self.intents.push_back(intent);
        Ok(())
    }

    pub(super) fn drain(&mut self) -> impl Iterator<Item = QueuedDropSlotIntent> + '_ {
        self.intents.drain(..)
    }
}
