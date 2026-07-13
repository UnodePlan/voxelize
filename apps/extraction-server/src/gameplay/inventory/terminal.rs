use super::{InventoryError, MatchInventory, ResourceStack};
use crate::match_world::RESOURCE_BACKPACK_SLOTS;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TerminalDrainProposal {
    inventory_revision: u32,
    slots: [Option<ResourceStack>; RESOURCE_BACKPACK_SLOTS],
}

impl TerminalDrainProposal {
    pub(crate) fn stacks(&self) -> impl Iterator<Item = ResourceStack> + '_ {
        self.slots.iter().copied().flatten()
    }
}

impl MatchInventory {
    /// 死亡排空先生成只读提案，外部资产准备成功后才能提交终态。
    pub(crate) fn propose_terminal_drain(&self) -> Result<TerminalDrainProposal, InventoryError> {
        if self.frozen {
            return Err(InventoryError::Frozen);
        }
        self.ensure_revision_available()?;
        Ok(TerminalDrainProposal {
            inventory_revision: self.revision,
            slots: self.slots,
        })
    }

    pub(crate) fn commit_terminal_drain(
        &mut self,
        proposal: TerminalDrainProposal,
    ) -> Result<(), InventoryError> {
        if self.frozen
            || self.revision != proposal.inventory_revision
            || self.slots != proposal.slots
        {
            return Err(InventoryError::ProposalStale);
        }
        self.ensure_revision_available()?;
        self.slots = [None; RESOURCE_BACKPACK_SLOTS];
        self.frozen = true;
        self.revision += 1;
        Ok(())
    }
}
