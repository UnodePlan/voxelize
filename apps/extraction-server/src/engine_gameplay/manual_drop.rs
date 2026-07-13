use std::{collections::BTreeMap, time::Duration};

use serde::Serialize;
use specs::{Entities, Entity, ReadStorage, WriteStorage};
use voxelize::{DirectionComp, MessageQueues, PositionComp};

use super::{
    authority::GameplayAuthority,
    components::{MatchPlayerComp, ResourceInventoryComp},
    intents::DropSlotIntentQueue,
    messaging::{queue_error, queue_ok},
    runtime::GameplayRuntimeContext,
};
use crate::{
    contracts::ErrorCode,
    gameplay::{
        drop_queue::{PendingDropQueue, SpawnedDropIds},
        inventory::InventoryError,
        transactions::{
            drop_slot_atomically, ManualDropAssets, ManualDropError, ManualDropRequest,
        },
    },
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DropSlotResult<'a> {
    drop_id: &'a str,
    inventory_revision: u32,
}

pub(super) struct ManualDropAccess<'a, 'world> {
    pub entities: &'a Entities<'world>,
    pub context: &'a GameplayRuntimeContext,
    pub authority: &'a GameplayAuthority,
    pub now: Duration,
    pub intents: &'a mut DropSlotIntentQueue,
    pub pending: &'a mut PendingDropQueue,
    pub spawned: &'a SpawnedDropIds,
    pub queues: &'a mut MessageQueues,
    pub players: &'a ReadStorage<'world, MatchPlayerComp>,
    pub inventories: &'a mut WriteStorage<'world, ResourceInventoryComp>,
    pub positions: &'a WriteStorage<'world, PositionComp>,
    pub directions: &'a ReadStorage<'world, DirectionComp>,
    pub dirty_players: &'a mut BTreeMap<String, Entity>,
}

pub(super) fn process_manual_drops(access: ManualDropAccess<'_, '_>) {
    let ManualDropAccess {
        entities,
        context,
        authority,
        now,
        intents,
        pending,
        spawned,
        queues,
        players,
        inventories,
        positions,
        directions,
        dirty_players,
    } = access;
    for intent in intents.drain().collect::<Vec<_>>() {
        let Some(player) = players.get(intent.entity) else {
            queue_error(
                queues,
                &context.manifest,
                &intent.client_id,
                intent.request_id,
                ErrorCode::GameInvalidState,
                false,
            );
            continue;
        };
        if !entities.is_alive(intent.entity)
            || player.public_player_id().to_string() != intent.client_id
            || !authority.allows(&intent.client_id, player.account_id())
        {
            queue_error(
                queues,
                &context.manifest,
                &intent.client_id,
                intent.request_id,
                ErrorCode::GameInvalidState,
                false,
            );
            continue;
        }
        let Some(inventory) = inventories.get_mut(intent.entity) else {
            queue_error(
                queues,
                &context.manifest,
                &intent.client_id,
                intent.request_id,
                ErrorCode::GameInvalidState,
                false,
            );
            continue;
        };
        let Some(position) = positions.get(intent.entity).map(|value| value.0.to_arr()) else {
            queue_error(
                queues,
                &context.manifest,
                &intent.client_id,
                intent.request_id,
                ErrorCode::GameInvalidState,
                false,
            );
            continue;
        };
        let Some(direction) = directions.get(intent.entity).map(|value| value.0.to_arr()) else {
            queue_error(
                queues,
                &context.manifest,
                &intent.client_id,
                intent.request_id,
                ErrorCode::GameInvalidState,
                false,
            );
            continue;
        };
        let result = drop_slot_atomically(
            ManualDropRequest {
                match_id: context.match_id,
                account_id: player.account_id(),
                seat_id: player.seat_id(),
                player_position: position,
                player_direction: direction,
                sequence: intent.sequence,
                intent: crate::gameplay::inventory::DropSlotIntent {
                    slot: intent.payload.slot,
                    expected_revision: intent.payload.expected_inventory_revision,
                },
                now,
            },
            &context.config,
            ManualDropAssets {
                inventory: inventory.inventory_mut(),
                pending,
                spawned,
            },
        );
        match result {
            Ok(receipt) => {
                dirty_players.insert(intent.client_id.clone(), intent.entity);
                queue_ok(
                    queues,
                    &context.manifest,
                    &intent.client_id,
                    intent.request_id,
                    DropSlotResult {
                        drop_id: receipt.id.as_str(),
                        inventory_revision: inventory.inventory().revision(),
                    },
                );
            }
            Err(error) => {
                let (code, retryable) = map_error(error);
                queue_error(
                    queues,
                    &context.manifest,
                    &intent.client_id,
                    intent.request_id,
                    code,
                    retryable,
                );
            }
        }
    }
}

fn map_error(error: ManualDropError) -> (ErrorCode, bool) {
    match error {
        ManualDropError::Inventory(InventoryError::StaleSequence)
        | ManualDropError::DuplicateDropId => (ErrorCode::GameStaleSequence, false),
        ManualDropError::Inventory(
            InventoryError::RevisionMismatch | InventoryError::ProposalStale,
        ) => (ErrorCode::GameStaleRevision, false),
        ManualDropError::Inventory(InventoryError::SlotOutOfRange | InventoryError::EmptySlot) => {
            (ErrorCode::InventorySlotInvalid, false)
        }
        ManualDropError::Inventory(InventoryError::Frozen) | ManualDropError::InvalidTransform => {
            (ErrorCode::GameInvalidState, false)
        }
        ManualDropError::Inventory(
            InventoryError::RevisionExhausted | InventoryError::InvalidMaxStack,
        )
        | ManualDropError::Loot(_)
        | ManualDropError::Pending(_)
        | ManualDropError::TimeOverflow => (ErrorCode::ServiceUnavailable, true),
    }
}
