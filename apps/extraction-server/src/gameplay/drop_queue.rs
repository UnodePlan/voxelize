use std::collections::{BTreeMap, BTreeSet};

use super::loot::{DropId, LootDrop};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EnqueueOutcome {
    Inserted,
    AlreadyPresent,
}

#[derive(Default)]
pub(crate) struct PendingDropQueue(BTreeMap<DropId, LootDrop>);

impl PendingDropQueue {
    pub(crate) fn enqueue(&mut self, drop: LootDrop) -> Result<EnqueueOutcome, PendingDropError> {
        match self.0.get(drop.id()) {
            Some(existing) if existing == &drop => Ok(EnqueueOutcome::AlreadyPresent),
            Some(_) => Err(PendingDropError::ConflictingId),
            None => {
                self.0.insert(drop.id().clone(), drop);
                Ok(EnqueueOutcome::Inserted)
            }
        }
    }

    pub(crate) fn remove(&mut self, id: &DropId) -> Option<LootDrop> {
        self.0.remove(id)
    }

    pub(crate) fn drain_sorted(&mut self) -> Vec<LootDrop> {
        std::mem::take(&mut self.0).into_values().collect()
    }

    #[cfg(test)]
    pub(crate) fn total_quantity(&self) -> u64 {
        self.0.values().map(|drop| drop.contents().total()).sum()
    }
}

#[derive(Default)]
pub(crate) struct SpawnedDropIds(BTreeSet<DropId>);

impl SpawnedDropIds {
    pub(crate) fn contains(&self, id: &DropId) -> bool {
        self.0.contains(id)
    }

    pub(crate) fn insert(&mut self, id: DropId) -> bool {
        self.0.insert(id)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PendingDropError {
    ConflictingId,
}
