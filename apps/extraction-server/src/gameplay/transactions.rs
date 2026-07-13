use std::time::Duration;

use uuid::Uuid;

use super::{
    config::GameplayConfig,
    drop_queue::{EnqueueOutcome, PendingDropError, PendingDropQueue, SpawnedDropIds},
    inventory::{DropSlotIntent, InventoryError, MatchInventory, ResourceStack},
    loot::{DropId, LootDrop, LootError, PickupExclusion, ResourceBundle},
};
use crate::matchmaking::SeatId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ManualDropReceipt {
    pub id: DropId,
    pub stack: ResourceStack,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ManualDropRequest {
    pub match_id: Uuid,
    pub account_id: Uuid,
    pub seat_id: SeatId,
    pub player_position: [f32; 3],
    pub player_direction: [f32; 3],
    pub sequence: u32,
    pub intent: DropSlotIntent,
    pub now: Duration,
}

pub(crate) struct ManualDropAssets<'a> {
    pub inventory: &'a mut MatchInventory,
    pub pending: &'a mut PendingDropQueue,
    pub spawned: &'a SpawnedDropIds,
}

pub(crate) fn drop_slot_atomically(
    request: ManualDropRequest,
    config: &GameplayConfig,
    assets: ManualDropAssets<'_>,
) -> Result<ManualDropReceipt, ManualDropError> {
    let proposal = assets
        .inventory
        .propose_drop(request.sequence, request.intent)
        .map_err(ManualDropError::Inventory)?;
    let id = DropId::manual(request.match_id, request.seat_id.get(), proposal.sequence());
    if assets.spawned.contains(&id) {
        return Err(ManualDropError::DuplicateDropId);
    }

    let drop_position = manual_drop_position(
        request.player_position,
        request.player_direction,
        config.manual_drop_distance,
        config.manual_drop_height,
    )?;
    let exclusion_until = request
        .now
        .checked_add(config.manual_drop_exclusion)
        .ok_or(ManualDropError::TimeOverflow)?;
    let stack = proposal.stack();
    let drop = LootDrop::new(
        id.clone(),
        drop_position,
        ResourceBundle::from_stack(stack),
        Some(PickupExclusion {
            account_id: request.account_id,
            until: exclusion_until,
        }),
    )
    .map_err(ManualDropError::Loot)?;

    match assets
        .pending
        .enqueue(drop)
        .map_err(ManualDropError::Pending)?
    {
        EnqueueOutcome::AlreadyPresent => return Err(ManualDropError::DuplicateDropId),
        EnqueueOutcome::Inserted => {}
    }
    if let Err(error) = assets.inventory.commit_drop(proposal) {
        let removed = assets.pending.remove(&id);
        debug_assert!(removed.is_some());
        return Err(ManualDropError::Inventory(error));
    }
    Ok(ManualDropReceipt { id, stack })
}

fn manual_drop_position(
    position: [f32; 3],
    direction: [f32; 3],
    distance: f32,
    height: f32,
) -> Result<[f32; 3], ManualDropError> {
    if !position.into_iter().all(f32::is_finite)
        || !direction.into_iter().all(f32::is_finite)
        || !distance.is_finite()
        || distance < 0.0
        || !height.is_finite()
    {
        return Err(ManualDropError::InvalidTransform);
    }
    let horizontal_length = direction[0].hypot(direction[2]);
    let (forward_x, forward_z) = if horizontal_length > f32::EPSILON {
        (
            direction[0] / horizontal_length,
            direction[2] / horizontal_length,
        )
    } else {
        (0.0, 1.0)
    };
    Ok([
        position[0] + forward_x * distance,
        position[1] + height,
        position[2] + forward_z * distance,
    ])
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PickupCandidate {
    pub seat_id: SeatId,
    pub account_id: Uuid,
    pub position: [f32; 3],
}

pub(crate) fn ordered_pickup_candidates(
    drop: &LootDrop,
    candidates: &[PickupCandidate],
    radius: f32,
    now: Duration,
) -> Result<Vec<usize>, PickupOrderError> {
    if !radius.is_finite() || radius <= 0.0 {
        return Err(PickupOrderError::InvalidRadius);
    }
    let drop_position = drop.position();
    let radius_squared = radius * radius;
    let mut eligible = Vec::with_capacity(candidates.len());
    for (index, candidate) in candidates.iter().enumerate() {
        if !candidate.position.into_iter().all(f32::is_finite) {
            return Err(PickupOrderError::InvalidPosition);
        }
        if drop.excludes(candidate.account_id, now) {
            continue;
        }
        let distance_squared = squared_distance(drop_position, candidate.position);
        if distance_squared <= radius_squared {
            eligible.push((index, distance_squared, candidate.seat_id));
        }
    }
    eligible.sort_by(|left, right| {
        left.1
            .total_cmp(&right.1)
            .then_with(|| left.2.cmp(&right.2))
    });
    Ok(eligible.into_iter().map(|candidate| candidate.0).collect())
}

fn squared_distance(left: [f32; 3], right: [f32; 3]) -> f32 {
    let dx = left[0] - right[0];
    let dy = left[1] - right[1];
    let dz = left[2] - right[2];
    dx * dx + dy * dy + dz * dz
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ManualDropError {
    Inventory(InventoryError),
    Loot(LootError),
    Pending(PendingDropError),
    DuplicateDropId,
    InvalidTransform,
    TimeOverflow,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PickupOrderError {
    InvalidRadius,
    InvalidPosition,
}
