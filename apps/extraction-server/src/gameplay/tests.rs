use std::time::Duration;

use uuid::Uuid;

use super::{
    config::{GameplayConfig, GAMEPLAY_V1},
    drop_queue::{EnqueueOutcome, PendingDropError, PendingDropQueue, SpawnedDropIds},
    equipment::FixedEquipment,
    inventory::{DropSlotIntent, InsertOutcome, InventoryError, MatchInventory, ResourceStack},
    loot::{DropId, LootDrop, LootError, PickupExclusion, ResourceBundle},
    transactions::{
        drop_slot_atomically, ordered_pickup_candidates, ManualDropAssets, ManualDropError,
        ManualDropRequest, PickupCandidate,
    },
};
use crate::{
    contracts::{EquipmentKey, ResourceKey},
    matchmaking::SeatId,
};

fn inventory() -> MatchInventory {
    MatchInventory::new(64).unwrap()
}

fn seat(value: usize) -> SeatId {
    SeatId::try_from(value).unwrap()
}

#[test]
fn gameplay_config_and_fixed_equipment_are_versioned_and_separate() {
    assert_eq!(
        GameplayConfig::resolve("pvp-mvp-v1", "balance-v1"),
        Some(&GAMEPLAY_V1)
    );
    assert!(GameplayConfig::resolve("pvp-mvp-v2", "balance-v1").is_none());
    let equipment = FixedEquipment::standard().snapshot();
    assert_eq!(equipment.pickaxe, EquipmentKey::BasicPickaxe);
    assert_eq!(equipment.melee_weapon, EquipmentKey::BasicMeleeWeapon);
}

#[test]
fn insert_fills_existing_stack_before_stable_empty_slots() {
    let mut inventory = inventory();
    assert_eq!(
        inventory.insert(ResourceKey::Gold, 65).unwrap(),
        InsertOutcome {
            accepted: 65,
            remainder: 0,
        }
    );
    assert_eq!(inventory.revision(), 1);
    assert_eq!(
        inventory.snapshot().slots[..3],
        [
            Some(ResourceStack {
                resource: ResourceKey::Gold,
                quantity: 64,
            }),
            Some(ResourceStack {
                resource: ResourceKey::Gold,
                quantity: 1,
            }),
            None,
        ]
    );

    inventory.insert(ResourceKey::Gold, 63).unwrap();
    assert_eq!(inventory.snapshot().slots[1].unwrap().quantity, 64);
    assert!(inventory.snapshot().slots[2].is_none());
}

#[test]
fn full_inventory_accepts_only_real_capacity_without_destroying_remainder() {
    let mut inventory = inventory();
    assert_eq!(
        inventory.insert(ResourceKey::Dirt, 767).unwrap().accepted,
        767
    );
    let outcome = inventory.insert(ResourceKey::Dirt, 10).unwrap();
    assert_eq!(outcome.accepted, 1);
    assert_eq!(outcome.remainder, 9);
    assert_eq!(inventory.total_quantity(), 768);

    let full_revision = inventory.revision();
    let outcome = inventory.insert(ResourceKey::Gold, 1).unwrap();
    assert_eq!(outcome.accepted, 0);
    assert_eq!(outcome.remainder, 1);
    assert_eq!(inventory.revision(), full_revision);
}

#[test]
fn multi_resource_insert_is_committed_with_one_revision() {
    let mut inventory = inventory();
    let outcomes = inventory
        .insert_batch(&[
            (ResourceKey::Dirt, 65),
            (ResourceKey::Gold, 2),
            (ResourceKey::Diamond, 3),
        ])
        .unwrap();
    assert_eq!(
        outcomes.iter().map(|outcome| outcome.accepted).sum::<u32>(),
        70
    );
    assert_eq!(inventory.revision(), 1);
    assert_eq!(inventory.quantity(ResourceKey::Dirt), 65);
    assert_eq!(inventory.quantity(ResourceKey::Gold), 2);
    assert_eq!(inventory.quantity(ResourceKey::Diamond), 3);
}

#[test]
fn drop_slot_consumes_sequence_and_commits_exact_whole_stack_once() {
    let mut inventory = inventory();
    inventory.insert(ResourceKey::Diamond, 12).unwrap();
    let intent = DropSlotIntent {
        slot: 0,
        expected_revision: inventory.revision(),
    };
    let proposal = inventory.propose_drop(7, intent).unwrap();
    let dropped = inventory.commit_drop(proposal).unwrap();
    assert_eq!(dropped.quantity, 12);
    assert!(inventory.snapshot().slots[0].is_none());
    assert_eq!(inventory.revision(), 2);

    assert_eq!(
        inventory.propose_drop(7, intent),
        Err(InventoryError::StaleSequence)
    );
    assert_eq!(
        inventory.propose_drop(
            8,
            DropSlotIntent {
                slot: 0,
                expected_revision: 1,
            }
        ),
        Err(InventoryError::RevisionMismatch)
    );
    assert_eq!(
        inventory.propose_drop(
            8,
            DropSlotIntent {
                slot: 0,
                expected_revision: 2,
            }
        ),
        Err(InventoryError::StaleSequence)
    );
}

#[test]
fn frozen_inventory_rejects_all_asset_mutations() {
    let mut inventory = inventory();
    inventory.insert(ResourceKey::Gold, 1).unwrap();
    assert!(inventory.freeze().unwrap());
    assert!(!inventory.freeze().unwrap());
    assert_eq!(
        inventory.insert(ResourceKey::Gold, 1),
        Err(InventoryError::Frozen)
    );
    assert_eq!(inventory.quantity(ResourceKey::Gold), 1);
}

#[test]
fn loot_transfer_preserves_partial_remainder_and_exclusion_boundary() {
    let match_id = Uuid::from_u128(1);
    let owner = Uuid::from_u128(2);
    let mut inventory = inventory();
    inventory.insert(ResourceKey::Dirt, 767).unwrap();
    let id = DropId::manual(match_id, 0, 1);
    let mut drop = LootDrop::new(
        id,
        [0.0, 49.0, 0.0],
        ResourceBundle::from_stack(ResourceStack {
            resource: ResourceKey::Dirt,
            quantity: 10,
        }),
        Some(PickupExclusion {
            account_id: owner,
            until: Duration::from_secs(2),
        }),
    )
    .unwrap();

    assert!(drop.excludes(owner, Duration::from_millis(1_999)));
    assert!(!drop.excludes(owner, Duration::from_secs(2)));
    assert_eq!(drop.transfer_into(&mut inventory).unwrap(), 1);
    assert_eq!(drop.contents().quantity(ResourceKey::Dirt), 9);
    assert_eq!(
        u64::from(inventory.total_quantity()) + drop.contents().total(),
        777
    );
}

#[test]
fn pending_and_spawned_ids_are_idempotent_and_unprotected_drops_merge() {
    let match_id = Uuid::from_u128(3);
    let id = DropId::manual(match_id, 0, 9);
    assert_eq!(
        id.as_str(),
        "drop:v1:00000000-0000-0000-0000-000000000003:seat:0:manual:9"
    );
    let first = LootDrop::new(
        id.clone(),
        [4.0, 49.0, 4.0],
        ResourceBundle::from_stack(ResourceStack {
            resource: ResourceKey::Gold,
            quantity: 5,
        }),
        None,
    )
    .unwrap();
    let duplicate = first.clone();
    let mut pending = PendingDropQueue::default();
    assert_eq!(pending.enqueue(first), Ok(EnqueueOutcome::Inserted));
    assert_eq!(
        pending.enqueue(duplicate),
        Ok(EnqueueOutcome::AlreadyPresent)
    );
    assert_eq!(pending.total_quantity(), 5);

    let mut spawned = SpawnedDropIds::default();
    let mut drained = pending.drain_sorted();
    let mut world_drop = drained.pop().unwrap();
    assert!(spawned.insert(world_drop.id().clone()));
    assert!(!spawned.insert(id.clone()));
    assert!(spawned.contains(&id));

    let second = LootDrop::new(
        DropId::manual(match_id, 0, 10),
        [5.0, 49.0, 5.0],
        ResourceBundle::from_stack(ResourceStack {
            resource: ResourceKey::Diamond,
            quantity: 2,
        }),
        None,
    )
    .unwrap();
    assert!(world_drop
        .merge(second, 2.0, Duration::from_secs(0))
        .unwrap());
    assert_eq!(world_drop.contents().quantity(ResourceKey::Gold), 5);
    assert_eq!(world_drop.contents().quantity(ResourceKey::Diamond), 2);
    assert_eq!(world_drop.contents().total(), 7);
}

#[test]
fn conflicting_pending_id_is_rejected_without_overwriting_assets() {
    let id = DropId::manual(Uuid::from_u128(5), 1, 3);
    let first = LootDrop::new(
        id.clone(),
        [0.0, 1.0, 0.0],
        ResourceBundle::new(1, 0, 0),
        None,
    )
    .unwrap();
    let conflicting =
        LootDrop::new(id, [0.0, 1.0, 0.0], ResourceBundle::new(2, 0, 0), None).unwrap();
    let mut pending = PendingDropQueue::default();
    assert_eq!(pending.enqueue(first), Ok(EnqueueOutcome::Inserted));
    assert_eq!(
        pending.enqueue(conflicting),
        Err(PendingDropError::ConflictingId)
    );
    assert_eq!(pending.total_quantity(), 1);
}

#[test]
fn protected_drop_merges_only_after_exclusion_expires() {
    let match_id = Uuid::from_u128(6);
    let account = Uuid::from_u128(7);
    let mut protected = LootDrop::new(
        DropId::manual(match_id, 0, 1),
        [0.0, 1.0, 0.0],
        ResourceBundle::new(0, 1, 0),
        Some(PickupExclusion {
            account_id: account,
            until: Duration::from_secs(2),
        }),
    )
    .unwrap();
    let public = LootDrop::new(
        DropId::manual(match_id, 1, 1),
        [0.5, 1.0, 0.5],
        ResourceBundle::new(0, 0, 1),
        None,
    )
    .unwrap();
    assert!(!protected
        .merge(public.clone(), 2.0, Duration::from_millis(1_999))
        .unwrap());
    assert!(protected
        .merge(public, 2.0, Duration::from_secs(2))
        .unwrap());
    assert_eq!(protected.contents().total(), 2);
}

#[test]
fn multi_resource_pickup_updates_both_owners_once_and_preserves_total() {
    let mut inventory = inventory();
    let mut drop = LootDrop::new(
        DropId::manual(Uuid::from_u128(8), 0, 1),
        [0.0, 1.0, 0.0],
        ResourceBundle::new(3, 4, 5),
        None,
    )
    .unwrap();
    assert_eq!(drop.transfer_into(&mut inventory).unwrap(), 12);
    assert_eq!(inventory.revision(), 1);
    assert_eq!(drop.revision(), 1);
    assert!(drop.is_empty());
    assert_eq!(
        u64::from(inventory.total_quantity()) + drop.contents().total(),
        12
    );
}

#[test]
fn pickup_order_uses_distance_then_seat_and_skips_excluded_owner() {
    let owner = Uuid::from_u128(9);
    let other = Uuid::from_u128(10);
    let drop = LootDrop::new(
        DropId::manual(Uuid::from_u128(11), 0, 1),
        [0.0, 0.0, 0.0],
        ResourceBundle::new(0, 1, 0),
        Some(PickupExclusion {
            account_id: owner,
            until: Duration::from_secs(2),
        }),
    )
    .unwrap();
    let candidates = [
        PickupCandidate {
            seat_id: seat(0),
            account_id: owner,
            position: [0.25, 0.0, 0.0],
        },
        PickupCandidate {
            seat_id: seat(2),
            account_id: other,
            position: [1.0, 0.0, 0.0],
        },
        PickupCandidate {
            seat_id: seat(1),
            account_id: Uuid::from_u128(12),
            position: [-1.0, 0.0, 0.0],
        },
    ];
    assert_eq!(
        ordered_pickup_candidates(&drop, &candidates, 2.0, Duration::from_secs(1)).unwrap(),
        vec![2, 1]
    );
    assert_eq!(
        ordered_pickup_candidates(&drop, &candidates, 2.0, Duration::from_secs(2)).unwrap(),
        vec![0, 2, 1]
    );
}

#[test]
fn full_nearest_player_cannot_block_next_pickup_candidate() {
    let mut inventories = [inventory(), inventory()];
    inventories[0].insert(ResourceKey::Dirt, 768).unwrap();
    let candidates = [
        PickupCandidate {
            seat_id: seat(0),
            account_id: Uuid::from_u128(13),
            position: [0.25, 0.0, 0.0],
        },
        PickupCandidate {
            seat_id: seat(1),
            account_id: Uuid::from_u128(14),
            position: [0.5, 0.0, 0.0],
        },
    ];
    let mut drop = LootDrop::new(
        DropId::manual(Uuid::from_u128(15), 2, 1),
        [0.0, 0.0, 0.0],
        ResourceBundle::new(0, 10, 0),
        None,
    )
    .unwrap();
    let order = ordered_pickup_candidates(&drop, &candidates, 2.0, Duration::ZERO).unwrap();
    for index in order {
        drop.transfer_into(&mut inventories[index]).unwrap();
        if drop.is_empty() {
            break;
        }
    }
    assert_eq!(inventories[0].quantity(ResourceKey::Gold), 0);
    assert_eq!(inventories[1].quantity(ResourceKey::Gold), 10);
    assert!(drop.is_empty());
}

#[test]
fn closer_player_wins_shared_drop_once() {
    let mut inventories = [inventory(), inventory()];
    let candidates = [
        PickupCandidate {
            seat_id: seat(1),
            account_id: Uuid::from_u128(19),
            position: [0.5, 0.0, 0.0],
        },
        PickupCandidate {
            seat_id: seat(0),
            account_id: Uuid::from_u128(20),
            position: [1.0, 0.0, 0.0],
        },
    ];
    let mut drop = LootDrop::new(
        DropId::manual(Uuid::from_u128(21), 2, 1),
        [0.0, 0.0, 0.0],
        ResourceBundle::new(0, 0, 9),
        None,
    )
    .unwrap();
    let order = ordered_pickup_candidates(&drop, &candidates, 2.0, Duration::ZERO).unwrap();
    for index in order {
        drop.transfer_into(&mut inventories[index]).unwrap();
        if drop.is_empty() {
            break;
        }
    }
    assert_eq!(inventories[0].quantity(ResourceKey::Diamond), 9);
    assert_eq!(inventories[1].quantity(ResourceKey::Diamond), 0);
    assert_eq!(drop.contents().total(), 0);
}

#[test]
fn whole_slot_drop_moves_assets_to_pending_exactly_once() {
    let match_id = Uuid::from_u128(16);
    let account_id = Uuid::from_u128(17);
    let mut inventory = inventory();
    inventory.insert(ResourceKey::Diamond, 17).unwrap();
    let mut pending = PendingDropQueue::default();
    let spawned = SpawnedDropIds::default();
    let revision = inventory.revision();
    let receipt = drop_slot_atomically(
        ManualDropRequest {
            match_id,
            account_id,
            seat_id: seat(3),
            player_position: [1.0, 2.0, 3.0],
            player_direction: [1.0, 0.0, 0.0],
            sequence: 7,
            intent: DropSlotIntent {
                slot: 0,
                expected_revision: revision,
            },
            now: Duration::from_secs(5),
        },
        &GAMEPLAY_V1,
        ManualDropAssets {
            inventory: &mut inventory,
            pending: &mut pending,
            spawned: &spawned,
        },
    )
    .unwrap();
    assert_eq!(receipt.stack.quantity, 17);
    assert_eq!(inventory.total_quantity(), 0);
    assert_eq!(pending.total_quantity(), 17);
    assert_eq!(inventory.revision(), revision + 1);

    assert_eq!(
        drop_slot_atomically(
            ManualDropRequest {
                match_id,
                account_id,
                seat_id: seat(3),
                player_position: [1.0, 2.0, 3.0],
                player_direction: [1.0, 0.0, 0.0],
                sequence: 7,
                intent: DropSlotIntent {
                    slot: 0,
                    expected_revision: inventory.revision(),
                },
                now: Duration::from_secs(5),
            },
            &GAMEPLAY_V1,
            ManualDropAssets {
                inventory: &mut inventory,
                pending: &mut pending,
                spawned: &spawned,
            },
        ),
        Err(ManualDropError::Inventory(InventoryError::StaleSequence))
    );
    assert_eq!(pending.total_quantity(), 17);
}

#[test]
fn invalid_drop_geometry_and_merge_overflow_leave_assets_unchanged() {
    assert_eq!(
        LootDrop::new(
            DropId::manual(Uuid::from_u128(18), 0, 1),
            [f32::NAN, 0.0, 0.0],
            ResourceBundle::new(1, 0, 0),
            None,
        ),
        Err(LootError::InvalidPosition)
    );
    let mut first = LootDrop::new(
        DropId::manual(Uuid::from_u128(18), 0, 2),
        [0.0, 0.0, 0.0],
        ResourceBundle::new(u32::MAX, 0, 0),
        None,
    )
    .unwrap();
    let second = LootDrop::new(
        DropId::manual(Uuid::from_u128(18), 1, 2),
        [0.0, 0.0, 0.0],
        ResourceBundle::new(1, 0, 0),
        None,
    )
    .unwrap();
    assert_eq!(
        first.merge(second, 2.0, Duration::ZERO),
        Err(LootError::QuantityOverflow)
    );
    assert_eq!(first.contents().quantity(ResourceKey::Dirt), u32::MAX);
    assert_eq!(first.revision(), 0);
}
