use uuid::Uuid;

use super::{
    config::GAMEPLAY_V1,
    drop_queue::{PendingDropError, PendingDropQueue, SpawnedDropIds},
    harvest::{
        award_harvest_atomically, HarvestAssets, HarvestDestination, HarvestError, HarvestRequest,
        HarvestedVoxelSet,
    },
    inventory::{MatchInventory, ResourceStack},
    loot::{DropId, LootDrop, ResourceBundle},
    mining::VoxelCoordinate,
};
use crate::{contracts::ResourceKey, match_world::RESOURCE_BACKPACK_SLOTS};

const MATCH_ID: Uuid = Uuid::from_u128(0x1234);

fn request(voxel: VoxelCoordinate, resource: ResourceKey) -> HarvestRequest {
    HarvestRequest {
        match_id: MATCH_ID,
        voxel,
        resource,
    }
}

fn award(
    request: HarvestRequest,
    harvested: &mut HarvestedVoxelSet,
    inventory: &mut MatchInventory,
    pending: &mut PendingDropQueue,
    spawned: &SpawnedDropIds,
) -> Result<HarvestDestination, HarvestError> {
    award_harvest_atomically(
        request,
        HarvestAssets {
            harvested,
            inventory,
            pending,
            spawned,
        },
    )
}

fn custodied_quantity(inventory: &MatchInventory, pending: &PendingDropQueue) -> u64 {
    u64::from(inventory.total_quantity()) + pending.total_quantity()
}

#[test]
fn harvest_claims_once_and_awards_exactly_one_inventory_item() {
    let voxel = VoxelCoordinate::new(1, 2, 3);
    let request = request(voxel, ResourceKey::Gold);
    let mut harvested = HarvestedVoxelSet::default();
    let mut inventory = MatchInventory::new(GAMEPLAY_V1.max_stack).unwrap();
    let mut pending = PendingDropQueue::default();
    let spawned = SpawnedDropIds::default();

    assert_eq!(
        award(
            request,
            &mut harvested,
            &mut inventory,
            &mut pending,
            &spawned,
        ),
        Ok(HarvestDestination::Inventory {
            inventory_revision: 1,
        })
    );
    assert!(harvested.contains(voxel));
    assert_eq!(inventory.quantity(ResourceKey::Gold), 1);
    assert_eq!(custodied_quantity(&inventory, &pending), 1);

    assert_eq!(
        award(
            request,
            &mut harvested,
            &mut inventory,
            &mut pending,
            &spawned,
        ),
        Err(HarvestError::AlreadyHarvested),
    );
    assert_eq!(inventory.revision(), 1);
    assert_eq!(custodied_quantity(&inventory, &pending), 1);
}

#[test]
fn full_inventory_routes_harvest_to_pending_without_losing_assets() {
    let voxel = VoxelCoordinate::new(4, 5, 6);
    let request = request(voxel, ResourceKey::Diamond);
    let mut harvested = HarvestedVoxelSet::default();
    let mut inventory = MatchInventory::new(GAMEPLAY_V1.max_stack).unwrap();
    let full_quantity = RESOURCE_BACKPACK_SLOTS as u32 * GAMEPLAY_V1.max_stack;
    assert_eq!(
        inventory
            .insert(ResourceKey::Dirt, full_quantity)
            .unwrap()
            .accepted,
        full_quantity,
    );
    let inventory_revision = inventory.revision();
    let quantity_before = custodied_quantity(&inventory, &PendingDropQueue::default());
    let mut pending = PendingDropQueue::default();
    let spawned = SpawnedDropIds::default();
    let expected_id = DropId::mined(MATCH_ID, voxel.x, voxel.y, voxel.z);

    assert_eq!(
        award(
            request,
            &mut harvested,
            &mut inventory,
            &mut pending,
            &spawned,
        ),
        Ok(HarvestDestination::Pending {
            drop_id: expected_id.clone(),
        })
    );
    assert!(harvested.contains(voxel));
    assert_eq!(inventory.revision(), inventory_revision);
    assert_eq!(inventory.total_quantity(), full_quantity);
    assert_eq!(
        custodied_quantity(&inventory, &pending),
        quantity_before + 1
    );

    let drops = pending.drain_sorted();
    assert_eq!(drops.len(), 1);
    assert_eq!(drops[0].id(), &expected_id);
    assert_eq!(drops[0].contents().quantity(ResourceKey::Diamond), 1);
}

#[test]
fn exhausted_inventory_revision_falls_back_to_pending_atomically() {
    let voxel = VoxelCoordinate::new(7, 8, 9);
    let request = request(voxel, ResourceKey::Gold);
    let mut harvested = HarvestedVoxelSet::default();
    let mut inventory = MatchInventory::new(GAMEPLAY_V1.max_stack).unwrap();
    inventory.set_revision_for_test(u32::MAX);
    let inventory_before = inventory.clone();
    let mut pending = PendingDropQueue::default();
    let spawned = SpawnedDropIds::default();
    let expected_id = DropId::mined(MATCH_ID, voxel.x, voxel.y, voxel.z);

    assert_eq!(
        award(
            request,
            &mut harvested,
            &mut inventory,
            &mut pending,
            &spawned,
        ),
        Ok(HarvestDestination::Pending {
            drop_id: expected_id,
        })
    );
    assert_eq!(inventory, inventory_before);
    assert!(harvested.contains(voxel));
    assert_eq!(custodied_quantity(&inventory, &pending), 1);
}

#[test]
fn pending_failure_rolls_back_claim_after_inventory_revision_exhaustion() {
    let voxel = VoxelCoordinate::new(10, 11, 12);
    let request = request(voxel, ResourceKey::Gold);
    let expected_id = DropId::mined(MATCH_ID, voxel.x, voxel.y, voxel.z);
    let conflicting_drop = LootDrop::new(
        expected_id,
        [
            voxel.x as f32 + 0.5,
            voxel.y as f32 + 0.5,
            voxel.z as f32 + 0.5,
        ],
        ResourceBundle::from_stack(ResourceStack {
            resource: ResourceKey::Dirt,
            quantity: 1,
        }),
        None,
    )
    .unwrap();

    let mut harvested = HarvestedVoxelSet::default();
    let mut inventory = MatchInventory::new(GAMEPLAY_V1.max_stack).unwrap();
    inventory.set_revision_for_test(u32::MAX);
    let inventory_before = inventory.clone();
    let mut pending = PendingDropQueue::default();
    pending.enqueue(conflicting_drop).unwrap();
    let spawned = SpawnedDropIds::default();
    let conserved_before = custodied_quantity(&inventory, &pending) + 1;

    assert_eq!(
        award(
            request,
            &mut harvested,
            &mut inventory,
            &mut pending,
            &spawned,
        ),
        Err(HarvestError::Pending(PendingDropError::ConflictingId)),
    );
    assert_eq!(inventory, inventory_before);
    assert!(!harvested.contains(voxel));
    assert_eq!(
        custodied_quantity(&inventory, &pending) + u64::from(!harvested.contains(voxel)),
        conserved_before,
    );
}
