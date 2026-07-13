use uuid::Uuid;

use super::{
    death::{
        drop_inventory_on_death_atomically, DeathDropAssets, DeathDropError, DeathDropRequest,
    },
    drop_queue::{PendingDropQueue, SpawnedDropIds},
    inventory::{InventoryError, MatchInventory},
    loot::{DropId, LootDrop, LootError, ResourceBundle},
};
use crate::{contracts::ResourceKey, matchmaking::SeatId};

fn request(match_id: Uuid) -> DeathDropRequest {
    DeathDropRequest {
        match_id,
        seat_id: SeatId::try_from(2).unwrap(),
        position: [4.0, 8.0, 12.0],
    }
}

#[test]
fn death_moves_all_inventory_assets_to_one_deterministic_drop() {
    let match_id = Uuid::new_v4();
    let mut inventory = MatchInventory::new(64).unwrap();
    inventory.insert(ResourceKey::Dirt, 70).unwrap();
    inventory.insert(ResourceKey::Gold, 5).unwrap();
    inventory.insert(ResourceKey::Diamond, 2).unwrap();
    let quantity_before = inventory.total_quantity() as u64;
    let revision_before = inventory.revision();
    let mut pending = PendingDropQueue::default();
    let spawned = SpawnedDropIds::default();

    let receipt = drop_inventory_on_death_atomically(
        request(match_id),
        DeathDropAssets {
            inventory: &mut inventory,
            pending: &mut pending,
            spawned: &spawned,
        },
    )
    .unwrap();

    assert!(inventory.is_frozen());
    assert_eq!(inventory.total_quantity(), 0);
    assert_eq!(inventory.revision(), revision_before + 1);
    assert_eq!(receipt.inventory_revision, inventory.revision());
    assert_eq!(receipt.contents.total(), quantity_before);
    assert_eq!(pending.total_quantity(), quantity_before);
    assert_eq!(
        receipt.drop_id.as_ref().unwrap().as_str(),
        format!("drop:v1:{match_id}:seat:2:death")
    );

    let drops = pending.drain_sorted();
    assert_eq!(drops.len(), 1);
    assert_eq!(drops[0].id(), receipt.drop_id.as_ref().unwrap());
    assert_eq!(drops[0].contents(), receipt.contents);
    assert_eq!(drops[0].position(), [4.0, 8.0, 12.0]);
}

#[test]
fn empty_death_still_freezes_inventory_without_creating_drop() {
    let mut inventory = MatchInventory::new(64).unwrap();
    let mut pending = PendingDropQueue::default();
    let spawned = SpawnedDropIds::default();
    let receipt = drop_inventory_on_death_atomically(
        request(Uuid::new_v4()),
        DeathDropAssets {
            inventory: &mut inventory,
            pending: &mut pending,
            spawned: &spawned,
        },
    )
    .unwrap();

    assert!(inventory.is_frozen());
    assert_eq!(inventory.revision(), 1);
    assert!(receipt.drop_id.is_none());
    assert!(receipt.contents.is_empty());
    assert_eq!(pending.total_quantity(), 0);
}

#[test]
fn repeated_death_cannot_duplicate_assets() {
    let match_id = Uuid::new_v4();
    let mut inventory = MatchInventory::new(64).unwrap();
    inventory.insert(ResourceKey::Gold, 5).unwrap();
    let mut pending = PendingDropQueue::default();
    let spawned = SpawnedDropIds::default();
    drop_inventory_on_death_atomically(
        request(match_id),
        DeathDropAssets {
            inventory: &mut inventory,
            pending: &mut pending,
            spawned: &spawned,
        },
    )
    .unwrap();
    let after_first = inventory.snapshot();

    assert_eq!(
        drop_inventory_on_death_atomically(
            request(match_id),
            DeathDropAssets {
                inventory: &mut inventory,
                pending: &mut pending,
                spawned: &spawned,
            },
        ),
        Err(DeathDropError::Inventory(InventoryError::Frozen))
    );
    assert_eq!(inventory.snapshot(), after_first);
    assert_eq!(pending.total_quantity(), 5);
}

#[test]
fn spawned_or_pending_id_conflict_leaves_inventory_unchanged() {
    let match_id = Uuid::new_v4();
    let drop_id = DropId::death(match_id, 2);
    let mut spawned = SpawnedDropIds::default();
    assert!(spawned.insert(drop_id.clone()));
    let mut inventory = MatchInventory::new(64).unwrap();
    inventory.insert(ResourceKey::Diamond, 2).unwrap();
    let before_spawned_conflict = inventory.snapshot();
    let mut pending = PendingDropQueue::default();

    assert_eq!(
        drop_inventory_on_death_atomically(
            request(match_id),
            DeathDropAssets {
                inventory: &mut inventory,
                pending: &mut pending,
                spawned: &spawned,
            },
        ),
        Err(DeathDropError::DuplicateDropId)
    );
    assert_eq!(inventory.snapshot(), before_spawned_conflict);
    assert_eq!(pending.total_quantity(), 0);

    let empty_spawned = SpawnedDropIds::default();
    pending
        .enqueue(
            LootDrop::new(
                drop_id,
                [4.0, 8.0, 12.0],
                ResourceBundle::new(1, 0, 0),
                None,
            )
            .unwrap(),
        )
        .unwrap();
    assert_eq!(
        drop_inventory_on_death_atomically(
            request(match_id),
            DeathDropAssets {
                inventory: &mut inventory,
                pending: &mut pending,
                spawned: &empty_spawned,
            },
        ),
        Err(DeathDropError::Pending(
            super::drop_queue::PendingDropError::ConflictingId
        ))
    );
    assert_eq!(inventory.snapshot(), before_spawned_conflict);
    assert_eq!(pending.total_quantity(), 1);
}

#[test]
fn invalid_position_and_revision_exhaustion_do_not_move_assets() {
    let mut inventory = MatchInventory::new(64).unwrap();
    inventory.insert(ResourceKey::Gold, 3).unwrap();
    let before_invalid_position = inventory.snapshot();
    let mut pending = PendingDropQueue::default();
    let spawned = SpawnedDropIds::default();
    let mut invalid_request = request(Uuid::new_v4());
    invalid_request.position[0] = f32::NAN;
    assert_eq!(
        drop_inventory_on_death_atomically(
            invalid_request,
            DeathDropAssets {
                inventory: &mut inventory,
                pending: &mut pending,
                spawned: &spawned,
            },
        ),
        Err(DeathDropError::Loot(LootError::InvalidPosition))
    );
    assert_eq!(inventory.snapshot(), before_invalid_position);

    inventory.set_revision_for_test(u32::MAX);
    let before_revision_overflow = inventory.snapshot();
    assert_eq!(
        drop_inventory_on_death_atomically(
            request(Uuid::new_v4()),
            DeathDropAssets {
                inventory: &mut inventory,
                pending: &mut pending,
                spawned: &spawned,
            },
        ),
        Err(DeathDropError::Inventory(InventoryError::RevisionExhausted))
    );
    assert_eq!(inventory.snapshot(), before_revision_overflow);
    assert_eq!(pending.total_quantity(), 0);
}

#[test]
fn terminal_drain_proposal_rejects_stale_inventory() {
    let mut inventory = MatchInventory::new(64).unwrap();
    inventory.insert(ResourceKey::Dirt, 3).unwrap();
    let proposal = inventory.propose_terminal_drain().unwrap();
    inventory.insert(ResourceKey::Gold, 2).unwrap();
    let before_commit = inventory.snapshot();

    assert_eq!(
        inventory.commit_terminal_drain(proposal),
        Err(InventoryError::ProposalStale)
    );
    assert_eq!(inventory.snapshot(), before_commit);
}
