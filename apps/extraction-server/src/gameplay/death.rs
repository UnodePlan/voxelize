use uuid::Uuid;

use super::{
    drop_queue::{EnqueueOutcome, PendingDropError, PendingDropQueue, SpawnedDropIds},
    inventory::{InventoryError, MatchInventory},
    loot::{DropId, LootDrop, LootError, ResourceBundle},
};
use crate::matchmaking::SeatId;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DeathDropRequest {
    pub match_id: Uuid,
    pub seat_id: SeatId,
    pub position: [f32; 3],
}

pub(crate) struct DeathDropAssets<'a> {
    pub inventory: &'a mut MatchInventory,
    pub pending: &'a mut PendingDropQueue,
    pub spawned: &'a SpawnedDropIds,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct DeathDropReceipt {
    pub drop_id: Option<DropId>,
    pub contents: ResourceBundle,
    pub inventory_revision: u32,
}

/// 背包排空和待生成掉落必须一起成功；提交失败时撤回队列写入。
pub(crate) fn drop_inventory_on_death_atomically(
    request: DeathDropRequest,
    assets: DeathDropAssets<'_>,
) -> Result<DeathDropReceipt, DeathDropError> {
    if !request.position.into_iter().all(f32::is_finite) {
        return Err(DeathDropError::Loot(LootError::InvalidPosition));
    }

    let proposal = assets.inventory.propose_terminal_drain()?;
    let mut contents = ResourceBundle::default();
    for stack in proposal.stacks() {
        contents.checked_merge(ResourceBundle::from_stack(stack))?;
    }

    if contents.is_empty() {
        assets.inventory.commit_terminal_drain(proposal)?;
        return Ok(DeathDropReceipt {
            drop_id: None,
            contents,
            inventory_revision: assets.inventory.revision(),
        });
    }

    let drop_id = DropId::death(request.match_id, request.seat_id.get());
    if assets.spawned.contains(&drop_id) {
        return Err(DeathDropError::DuplicateDropId);
    }
    let drop = LootDrop::new(drop_id.clone(), request.position, contents, None)?;
    match assets.pending.enqueue(drop)? {
        EnqueueOutcome::Inserted => {}
        EnqueueOutcome::AlreadyPresent => return Err(DeathDropError::DuplicateDropId),
    }

    if let Err(error) = assets.inventory.commit_terminal_drain(proposal) {
        let removed = assets.pending.remove(&drop_id);
        debug_assert!(removed.is_some(), "死亡事务回滚时待生成掉落必须存在");
        return Err(DeathDropError::Inventory(error));
    }

    Ok(DeathDropReceipt {
        drop_id: Some(drop_id),
        contents,
        inventory_revision: assets.inventory.revision(),
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DeathDropError {
    Inventory(InventoryError),
    Loot(LootError),
    Pending(PendingDropError),
    DuplicateDropId,
}

impl From<InventoryError> for DeathDropError {
    fn from(error: InventoryError) -> Self {
        Self::Inventory(error)
    }
}

impl From<LootError> for DeathDropError {
    fn from(error: LootError) -> Self {
        Self::Loot(error)
    }
}

impl From<PendingDropError> for DeathDropError {
    fn from(error: PendingDropError) -> Self {
        Self::Pending(error)
    }
}
