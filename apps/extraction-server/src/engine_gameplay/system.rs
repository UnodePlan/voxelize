use std::collections::BTreeMap;

use specs::{Entities, ReadExpect, ReadStorage, System, WriteExpect, WriteStorage};
use voxelize::{
    CurrentChunkComp, DirectionComp, DoNotPersistComp, ETypeComp, EntityFlag, EntityIDs, IDComp,
    MessageQueues, MetadataComp, PositionComp,
};

use super::{
    authority::GameplayAuthority,
    auto_pickup::{auto_pickup, AutoPickupAccess},
    components::{
        EliminationComp, ExtractionComp, FixedEquipmentComp, HealthComp, LootDropComp,
        MatchPlayerComp, ResourceInventoryComp, RoundStatsComp,
    },
    death_outbox::flush_death_notices,
    drop_spawn::{spawn_pending_drops, DropSpawnAccess},
    extraction_outbox::{flush_extraction_notices, terminal_outboxes_are_flushed},
    intents::DropSlotIntentQueue,
    manual_drop::{process_manual_drops, ManualDropAccess},
    messaging::{queue_inventory_state, PlayerInventoryState},
    runtime::GameplayRuntimeContext,
    HardDeadlineControl,
};
use crate::gameplay::drop_queue::{PendingDropQueue, SpawnedDropIds};

pub(super) struct GameplayRuntimeSystem;

impl<'a> System<'a> for GameplayRuntimeSystem {
    type SystemData = (
        Entities<'a>,
        ReadExpect<'a, GameplayRuntimeContext>,
        ReadExpect<'a, GameplayAuthority>,
        ReadExpect<'a, HardDeadlineControl>,
        WriteExpect<'a, DropSlotIntentQueue>,
        WriteExpect<'a, PendingDropQueue>,
        WriteExpect<'a, SpawnedDropIds>,
        WriteExpect<'a, MessageQueues>,
        WriteExpect<'a, EntityIDs>,
        ReadStorage<'a, MatchPlayerComp>,
        ReadStorage<'a, FixedEquipmentComp>,
        ReadStorage<'a, HealthComp>,
        WriteStorage<'a, RoundStatsComp>,
        ReadStorage<'a, DirectionComp>,
        WriteStorage<'a, EliminationComp>,
        WriteStorage<'a, ExtractionComp>,
        WriteStorage<'a, ResourceInventoryComp>,
        WriteStorage<'a, PositionComp>,
        WriteStorage<'a, LootDropComp>,
        WriteStorage<'a, IDComp>,
        WriteStorage<'a, EntityFlag>,
        WriteStorage<'a, CurrentChunkComp>,
        WriteStorage<'a, ETypeComp>,
        WriteStorage<'a, MetadataComp>,
        WriteStorage<'a, DoNotPersistComp>,
    );

    fn run(&mut self, data: Self::SystemData) {
        let (
            entities,
            context,
            authority,
            hard_deadline,
            mut intents,
            mut pending,
            mut spawned,
            mut queues,
            mut entity_ids,
            players,
            equipment,
            health,
            mut stats,
            directions,
            mut eliminations,
            mut extractions,
            mut inventories,
            mut positions,
            mut loots,
            mut ids,
            mut flags,
            mut chunks,
            mut etypes,
            mut metadatas,
            mut no_persist,
        ) = data;
        let Some(now) = authority.monotonic_now() else {
            return;
        };
        let mut dirty_players = BTreeMap::new();

        flush_death_notices(
            &entities,
            &authority,
            context.match_id,
            &players,
            &stats,
            &mut eliminations,
        );
        flush_extraction_notices(&entities, &authority, &players, &mut extractions);
        if terminal_outboxes_are_flushed(&entities, &eliminations, &extractions) {
            hard_deadline.seal_after_outbox();
        }

        process_manual_drops(ManualDropAccess {
            entities: &entities,
            context: &context,
            authority: &authority,
            now,
            intents: &mut intents,
            pending: &mut pending,
            spawned: &spawned,
            queues: &mut queues,
            players: &players,
            health: &health,
            eliminations: &eliminations,
            extractions: &extractions,
            inventories: &mut inventories,
            positions: &positions,
            directions: &directions,
            dirty_players: &mut dirty_players,
        });
        spawn_pending_drops(DropSpawnAccess {
            entities: &entities,
            context: &context,
            now,
            pending: &mut pending,
            spawned: &mut spawned,
            entity_ids: &mut entity_ids,
            loots: &mut loots,
            ids: &mut ids,
            flags: &mut flags,
            chunks: &mut chunks,
            etypes: &mut etypes,
            metadatas: &mut metadatas,
            positions: &mut positions,
            no_persist: &mut no_persist,
        });
        auto_pickup(AutoPickupAccess {
            entities: &entities,
            authority: &authority,
            now,
            radius: context.config.pickup_radius,
            entity_ids: &mut entity_ids,
            players: &players,
            health: &health,
            eliminations: &eliminations,
            extractions: &extractions,
            inventories: &mut inventories,
            stats: &mut stats,
            positions: &positions,
            loots: &mut loots,
            metadatas: &mut metadatas,
            dirty_players: &mut dirty_players,
        });

        for (client_id, entity) in dirty_players {
            if let Some(state) = inventories
                .get(entity)
                .zip(equipment.get(entity))
                .map(|(inventory, equipment)| PlayerInventoryState::new(inventory, equipment))
            {
                queue_inventory_state(&mut queues, &client_id, &state);
            }
        }
    }
}
