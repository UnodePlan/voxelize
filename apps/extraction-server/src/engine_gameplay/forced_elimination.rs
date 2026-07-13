use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

use uuid::Uuid;

/// Matchmaking 线程只写共享有界意图，World ECS 在权威 tick 内决定唯一终态。
#[derive(Clone, Default)]
pub(crate) struct ForcedEliminationQueue(Arc<Mutex<BTreeSet<Uuid>>>);

impl ForcedEliminationQueue {
    const CAPACITY: usize = crate::matchmaking::MATCH_SIZE;

    pub(crate) fn enqueue(&self, account_id: Uuid) -> Result<bool, ForcedEliminationError> {
        if account_id.is_nil() {
            return Err(ForcedEliminationError::InvalidAccount);
        }
        let mut queue = self
            .0
            .lock()
            .map_err(|_| ForcedEliminationError::Poisoned)?;
        if queue.contains(&account_id) {
            return Ok(false);
        }
        if queue.len() >= Self::CAPACITY {
            return Err(ForcedEliminationError::Full);
        }
        queue.insert(account_id);
        Ok(true)
    }

    pub(super) fn drain(&self) -> Result<Vec<Uuid>, ForcedEliminationError> {
        let mut queue = self
            .0
            .lock()
            .map_err(|_| ForcedEliminationError::Poisoned)?;
        Ok(std::mem::take(&mut *queue).into_iter().collect())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ForcedEliminationError {
    InvalidAccount,
    Full,
    Poisoned,
}
