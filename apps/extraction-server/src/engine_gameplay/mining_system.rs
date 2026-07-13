use specs::{Entities, Join, ReadExpect, ReadStorage, System, WriteExpect, WriteStorage};
use voxelize::{Chunks, Clients, DirectionComp, MessageQueues, PositionComp};

use super::{
    authority::GameplayAuthority,
    components::{FixedEquipmentComp, MatchPlayerComp, MiningComp, ResourceInventoryComp},
    intents::MiningIntentQueue,
    messaging::{queue_error, queue_inventory_state, queue_mining_state},
    mining_completion::{advance_mining, MiningCompletionAccess},
    mining_dirty::MiningDirtyPlayers,
    mining_intents::{process_mining_intents, MiningIntentAccess},
    runtime::GameplayRuntimeContext,
};
use crate::{
    contracts::{ErrorCode, MiningIdleReason},
    gameplay::{
        drop_queue::{PendingDropQueue, SpawnedDropIds},
        harvest::HarvestedVoxelSet,
    },
};

pub(super) struct MiningResolutionSystem;

impl<'a> System<'a> for MiningResolutionSystem {
    type SystemData = (
        Entities<'a>,
        ReadExpect<'a, GameplayRuntimeContext>,
        ReadExpect<'a, GameplayAuthority>,
        ReadExpect<'a, Clients>,
        WriteExpect<'a, MiningIntentQueue>,
        WriteExpect<'a, HarvestedVoxelSet>,
        WriteExpect<'a, PendingDropQueue>,
        ReadExpect<'a, SpawnedDropIds>,
        WriteExpect<'a, MessageQueues>,
        WriteExpect<'a, Chunks>,
        ReadStorage<'a, MatchPlayerComp>,
        ReadStorage<'a, FixedEquipmentComp>,
        ReadStorage<'a, PositionComp>,
        ReadStorage<'a, DirectionComp>,
        WriteStorage<'a, MiningComp>,
        WriteStorage<'a, ResourceInventoryComp>,
    );

    fn run(&mut self, data: Self::SystemData) {
        let (
            entities,
            context,
            authority,
            clients,
            mut intents,
            mut harvested,
            mut pending,
            spawned,
            mut queues,
            mut chunks,
            players,
            equipment,
            positions,
            directions,
            mut mining,
            mut inventories,
        ) = data;
        let mut dirty = MiningDirtyPlayers::default();
        let Some(now) = authority.monotonic_now() else {
            fail_closed_without_clock(
                &entities,
                &context,
                &mut intents,
                &mut queues,
                &players,
                &mut mining,
                &mut dirty,
            );
            sync_dirty(
                dirty,
                &context,
                &mut queues,
                &equipment,
                &mining,
                &inventories,
            );
            return;
        };

        process_mining_intents(MiningIntentAccess {
            entities: &entities,
            context: &context,
            authority: &authority,
            now,
            clients: &clients,
            intents: &mut intents,
            queues: &mut queues,
            chunks: &chunks,
            harvested: &harvested,
            players: &players,
            equipment: &equipment,
            positions: &positions,
            directions: &directions,
            mining: &mut mining,
            dirty: &mut dirty,
        });
        advance_mining(MiningCompletionAccess {
            entities: &entities,
            context: &context,
            authority: &authority,
            clients: &clients,
            now,
            chunks: &mut chunks,
            harvested: &mut harvested,
            pending: &mut pending,
            spawned: &spawned,
            players: &players,
            equipment: &equipment,
            positions: &positions,
            directions: &directions,
            mining: &mut mining,
            inventories: &mut inventories,
            dirty: &mut dirty,
        });
        sync_dirty(
            dirty,
            &context,
            &mut queues,
            &equipment,
            &mining,
            &inventories,
        );
    }
}

fn fail_closed_without_clock<'a>(
    entities: &Entities<'a>,
    context: &GameplayRuntimeContext,
    intents: &mut MiningIntentQueue,
    queues: &mut MessageQueues,
    players: &ReadStorage<'a, MatchPlayerComp>,
    mining: &mut WriteStorage<'a, MiningComp>,
    dirty: &mut MiningDirtyPlayers,
) {
    for intent in intents.drain().collect::<Vec<_>>() {
        queue_error(
            queues,
            &context.manifest,
            &intent.client_id,
            intent.request_id,
            ErrorCode::ServiceUnavailable,
            true,
        );
    }
    for (entity, player, component) in (entities, players, mining).join() {
        if component
            .state_mut()
            .reset(MiningIdleReason::Disconnected)
            .unwrap_or(false)
        {
            dirty.mark(&player.public_player_id().to_string(), entity, false);
        }
    }
}

fn sync_dirty<'a>(
    dirty: MiningDirtyPlayers,
    context: &GameplayRuntimeContext,
    queues: &mut MessageQueues,
    equipment: &ReadStorage<'a, FixedEquipmentComp>,
    mining: &WriteStorage<'a, MiningComp>,
    inventories: &WriteStorage<'a, ResourceInventoryComp>,
) {
    for (client_id, dirty) in dirty.into_inner() {
        if let Some(component) = mining.get(dirty.entity) {
            queue_mining_state(queues, context, &client_id, component);
        }
        if dirty.inventory {
            if let Some((inventory, equipment)) = inventories
                .get(dirty.entity)
                .zip(equipment.get(dirty.entity))
            {
                queue_inventory_state(
                    queues,
                    &client_id,
                    &super::messaging::PlayerInventoryState::new(inventory, equipment),
                );
            }
        }
    }
}
