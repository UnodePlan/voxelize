use serde::{Deserialize, Serialize};

use crate::{contracts::ResourceKey, match_world::RESOURCE_BACKPACK_SLOTS};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceStack {
    pub resource: ResourceKey,
    pub quantity: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InsertOutcome {
    pub accepted: u32,
    pub remainder: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DropSlotIntent {
    pub slot: usize,
    pub expected_revision: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DropProposal {
    sequence: u32,
    slot: usize,
    inventory_revision: u32,
    stack: ResourceStack,
}

impl DropProposal {
    pub(crate) fn sequence(self) -> u32 {
        self.sequence
    }

    pub(crate) fn stack(self) -> ResourceStack {
        self.stack
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InventorySnapshot {
    pub slots: [Option<ResourceStack>; RESOURCE_BACKPACK_SLOTS],
    pub revision: u32,
    pub frozen: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MatchInventory {
    slots: [Option<ResourceStack>; RESOURCE_BACKPACK_SLOTS],
    revision: u32,
    frozen: bool,
    last_drop_sequence: Option<u32>,
    max_stack: u32,
}

impl MatchInventory {
    pub(crate) fn new(max_stack: u32) -> Result<Self, InventoryError> {
        if max_stack == 0 {
            return Err(InventoryError::InvalidMaxStack);
        }
        Ok(Self {
            slots: [None; RESOURCE_BACKPACK_SLOTS],
            revision: 0,
            frozen: false,
            last_drop_sequence: None,
            max_stack,
        })
    }

    pub(crate) fn snapshot(&self) -> InventorySnapshot {
        InventorySnapshot {
            slots: self.slots,
            revision: self.revision,
            frozen: self.frozen,
        }
    }

    pub(crate) fn revision(&self) -> u32 {
        self.revision
    }

    pub(crate) fn is_frozen(&self) -> bool {
        self.frozen
    }

    #[cfg(test)]
    pub(crate) fn insert(
        &mut self,
        resource: ResourceKey,
        quantity: u32,
    ) -> Result<InsertOutcome, InventoryError> {
        Ok(self.insert_batch(&[(resource, quantity)])?[0])
    }

    /// 多种资源先在副本上按稳定顺序试装，最后只增加一次 revision。
    pub(crate) fn insert_batch(
        &mut self,
        entries: &[(ResourceKey, u32)],
    ) -> Result<Vec<InsertOutcome>, InventoryError> {
        if self.frozen {
            return Err(InventoryError::Frozen);
        }

        let mut candidate = self.clone();
        let mut outcomes = Vec::with_capacity(entries.len());
        let mut changed = false;
        for &(resource, quantity) in entries {
            let outcome = candidate.insert_without_revision(resource, quantity);
            changed |= outcome.accepted > 0;
            outcomes.push(outcome);
        }
        if changed {
            self.ensure_revision_available()?;
            candidate.revision = self.revision + 1;
            *self = candidate;
        }
        Ok(outcomes)
    }

    fn insert_without_revision(&mut self, resource: ResourceKey, quantity: u32) -> InsertOutcome {
        let capacity = self.available_capacity(resource);
        let accepted = u64::from(quantity).min(capacity) as u32;
        if accepted == 0 {
            return InsertOutcome {
                accepted: 0,
                remainder: quantity,
            };
        }

        let mut remaining = accepted;
        for stack in self.slots.iter_mut().flatten() {
            if stack.resource != resource || stack.quantity >= self.max_stack {
                continue;
            }
            let added = remaining.min(self.max_stack - stack.quantity);
            stack.quantity += added;
            remaining -= added;
            if remaining == 0 {
                break;
            }
        }
        if remaining > 0 {
            for slot in &mut self.slots {
                if slot.is_some() {
                    continue;
                }
                let added = remaining.min(self.max_stack);
                *slot = Some(ResourceStack {
                    resource,
                    quantity: added,
                });
                remaining -= added;
                if remaining == 0 {
                    break;
                }
            }
        }
        debug_assert_eq!(remaining, 0);
        InsertOutcome {
            accepted,
            remainder: quantity - accepted,
        }
    }

    pub(crate) fn propose_drop(
        &mut self,
        sequence: u32,
        intent: DropSlotIntent,
    ) -> Result<DropProposal, InventoryError> {
        if self
            .last_drop_sequence
            .is_some_and(|last_sequence| sequence <= last_sequence)
        {
            return Err(InventoryError::StaleSequence);
        }
        self.last_drop_sequence = Some(sequence);
        if self.frozen {
            return Err(InventoryError::Frozen);
        }
        if intent.expected_revision != self.revision {
            return Err(InventoryError::RevisionMismatch);
        }
        let stack = self
            .slots
            .get(intent.slot)
            .ok_or(InventoryError::SlotOutOfRange)?
            .ok_or(InventoryError::EmptySlot)?;
        self.ensure_revision_available()?;
        Ok(DropProposal {
            sequence,
            slot: intent.slot,
            inventory_revision: self.revision,
            stack,
        })
    }

    pub(crate) fn commit_drop(
        &mut self,
        proposal: DropProposal,
    ) -> Result<ResourceStack, InventoryError> {
        if self.frozen
            || self.revision != proposal.inventory_revision
            || self.slots.get(proposal.slot).copied().flatten() != Some(proposal.stack)
        {
            return Err(InventoryError::ProposalStale);
        }
        self.ensure_revision_available()?;
        self.slots[proposal.slot] = None;
        self.revision += 1;
        Ok(proposal.stack)
    }

    #[cfg(test)]
    pub(crate) fn freeze(&mut self) -> Result<bool, InventoryError> {
        if self.frozen {
            return Ok(false);
        }
        self.ensure_revision_available()?;
        self.frozen = true;
        self.revision += 1;
        Ok(true)
    }

    #[cfg(test)]
    pub(crate) fn quantity(&self, resource: ResourceKey) -> u32 {
        self.slots
            .iter()
            .flatten()
            .filter(|stack| stack.resource == resource)
            .map(|stack| stack.quantity)
            .sum()
    }

    #[cfg(test)]
    pub(crate) fn total_quantity(&self) -> u32 {
        self.slots
            .iter()
            .flatten()
            .map(|stack| stack.quantity)
            .sum()
    }

    #[cfg(test)]
    pub(crate) fn set_revision_for_test(&mut self, revision: u32) {
        self.revision = revision;
    }

    fn available_capacity(&self, resource: ResourceKey) -> u64 {
        self.slots
            .iter()
            .map(|slot| match slot {
                Some(stack) if stack.resource == resource => {
                    u64::from(self.max_stack - stack.quantity)
                }
                None => u64::from(self.max_stack),
                _ => 0,
            })
            .sum()
    }

    fn ensure_revision_available(&self) -> Result<(), InventoryError> {
        if self.revision == u32::MAX {
            Err(InventoryError::RevisionExhausted)
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum InventoryError {
    InvalidMaxStack,
    Frozen,
    StaleSequence,
    RevisionMismatch,
    SlotOutOfRange,
    EmptySlot,
    ProposalStale,
    RevisionExhausted,
}
