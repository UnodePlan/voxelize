use std::collections::HashSet;

use uuid::Uuid;

use super::{
    drop_queue::{EnqueueOutcome, PendingDropError, PendingDropQueue, SpawnedDropIds},
    inventory::{InventoryError, MatchInventory},
    loot::{DropId, LootDrop, LootError, ResourceBundle},
    mining::VoxelCoordinate,
};
use crate::contracts::ResourceKey;

#[derive(Default)]
pub(crate) struct HarvestedVoxelSet(HashSet<VoxelCoordinate>);

impl HarvestedVoxelSet {
    pub(crate) fn contains(&self, voxel: VoxelCoordinate) -> bool {
        self.0.contains(&voxel)
    }

    fn claim(&mut self, voxel: VoxelCoordinate) -> bool {
        self.0.insert(voxel)
    }

    fn release(&mut self, voxel: VoxelCoordinate) {
        self.0.remove(&voxel);
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct HarvestRequest {
    pub match_id: Uuid,
    pub voxel: VoxelCoordinate,
    pub resource: ResourceKey,
}

pub(crate) struct HarvestAssets<'a> {
    pub harvested: &'a mut HarvestedVoxelSet,
    pub inventory: &'a mut MatchInventory,
    pub pending: &'a mut PendingDropQueue,
    pub spawned: &'a SpawnedDropIds,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum HarvestDestination {
    Inventory { inventory_revision: u32 },
    Pending { drop_id: DropId },
}

pub(crate) fn award_harvest_atomically(
    request: HarvestRequest,
    assets: HarvestAssets<'_>,
) -> Result<HarvestDestination, HarvestError> {
    if assets.inventory.is_frozen() {
        return Err(HarvestError::Inventory(InventoryError::Frozen));
    }
    // AIR 仍在 staging 时其他玩家还能读到旧方块，必须先 claim 才能保证只产出一次。
    if !assets.harvested.claim(request.voxel) {
        return Err(HarvestError::AlreadyHarvested);
    }

    let insertion = assets.inventory.insert_batch(&[(request.resource, 1)]);
    match insertion {
        Ok(outcomes) if outcomes[0].accepted == 1 && outcomes[0].remainder == 0 => {
            return Ok(HarvestDestination::Inventory {
                inventory_revision: assets.inventory.revision(),
            });
        }
        Ok(outcomes) if outcomes[0].accepted == 0 && outcomes[0].remainder == 1 => {}
        Err(InventoryError::RevisionExhausted) => {}
        Err(error) => {
            assets.harvested.release(request.voxel);
            return Err(HarvestError::Inventory(error));
        }
        Ok(_) => {
            assets.harvested.release(request.voxel);
            return Err(HarvestError::InvariantViolation);
        }
    }

    let id = DropId::mined(
        request.match_id,
        request.voxel.x,
        request.voxel.y,
        request.voxel.z,
    );
    if assets.spawned.contains(&id) {
        assets.harvested.release(request.voxel);
        return Err(HarvestError::DuplicateDropId);
    }
    let drop = match LootDrop::new(
        id.clone(),
        [
            request.voxel.x as f32 + 0.5,
            request.voxel.y as f32 + 0.5,
            request.voxel.z as f32 + 0.5,
        ],
        ResourceBundle::from_stack(super::inventory::ResourceStack {
            resource: request.resource,
            quantity: 1,
        }),
        None,
    ) {
        Ok(drop) => drop,
        Err(error) => {
            assets.harvested.release(request.voxel);
            return Err(HarvestError::Loot(error));
        }
    };
    let enqueue = match assets.pending.enqueue(drop) {
        Ok(outcome) => outcome,
        Err(error) => {
            assets.harvested.release(request.voxel);
            return Err(HarvestError::Pending(error));
        }
    };
    match enqueue {
        EnqueueOutcome::Inserted => Ok(HarvestDestination::Pending { drop_id: id }),
        EnqueueOutcome::AlreadyPresent => {
            assets.harvested.release(request.voxel);
            Err(HarvestError::DuplicateDropId)
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HarvestError {
    AlreadyHarvested,
    DuplicateDropId,
    InvariantViolation,
    Inventory(InventoryError),
    Pending(PendingDropError),
    Loot(LootError),
}
