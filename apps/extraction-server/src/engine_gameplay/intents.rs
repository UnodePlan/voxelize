use std::collections::VecDeque;

use specs::Entity;
use uuid::Uuid;

use crate::contracts::{AttackPayload, DropSlotPayload, MiningPayload};

#[derive(Debug)]
pub(super) struct QueuedDropSlotIntent {
    pub entity: Entity,
    pub client_id: String,
    pub request_id: Uuid,
    pub sequence: u32,
    pub payload: DropSlotPayload,
}

#[derive(Debug)]
pub(super) struct QueuedMiningIntent {
    pub entity: Entity,
    pub client_id: String,
    pub request_id: Uuid,
    pub sequence: u32,
    pub payload: MiningPayload,
}

#[derive(Debug)]
pub(super) struct QueuedAttackIntent {
    pub entity: Entity,
    pub client_id: String,
    pub request_id: Uuid,
    pub sequence: u32,
    pub payload: AttackPayload,
}

pub(super) type DropSlotIntentQueue = BoundedIntentQueue<QueuedDropSlotIntent>;
pub(super) type MiningIntentQueue = BoundedIntentQueue<QueuedMiningIntent>;
pub(super) type AttackIntentQueue = BoundedIntentQueue<QueuedAttackIntent>;

pub(super) struct BoundedIntentQueue<T> {
    capacity: usize,
    intents: VecDeque<T>,
}

impl<T> BoundedIntentQueue<T> {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            intents: VecDeque::with_capacity(capacity),
        }
    }

    pub(super) fn push(&mut self, intent: T) -> Result<(), T> {
        if self.intents.len() >= self.capacity {
            return Err(intent);
        }
        self.intents.push_back(intent);
        Ok(())
    }

    pub(super) fn drain(&mut self) -> impl Iterator<Item = T> + '_ {
        self.intents.drain(..)
    }
}
