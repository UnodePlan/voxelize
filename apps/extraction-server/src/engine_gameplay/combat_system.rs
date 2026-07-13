use specs::{Entities, ReadExpect, ReadStorage, System, WriteExpect, WriteStorage};
use voxelize::{Chunks, Clients, DirectionComp, MessageQueues, PositionComp, Registry};

use super::{
    authority::GameplayAuthority,
    combat_attacks::{resolve_attacks, AttackResolutionAccess},
    combat_responses::reject_all_unavailable,
    components::{
        CombatComp, EliminationComp, ExtractionComp, FixedEquipmentComp, HealthComp,
        MatchPlayerComp, MiningComp, ResourceInventoryComp, RoundStatsComp,
    },
    extraction_resolution::{process_extractions, ExtractionResolutionAccess},
    intents::AttackIntentQueue,
    runtime::GameplayRuntimeContext,
    timeout_resolution::{
        process_forced_eliminations, process_hard_deadline_eliminations, TimeoutResolutionAccess,
    },
    ForcedEliminationQueue, HardDeadlineControl,
};
use crate::{
    gameplay::drop_queue::{PendingDropQueue, SpawnedDropIds},
    generation::MapPoint,
};

pub(super) struct CombatResolutionSystem;

impl<'a> System<'a> for CombatResolutionSystem {
    type SystemData = (
        Entities<'a>,
        ReadExpect<'a, GameplayRuntimeContext>,
        ReadExpect<'a, GameplayAuthority>,
        ReadExpect<'a, Clients>,
        ReadExpect<'a, Chunks>,
        ReadExpect<'a, Registry>,
        ReadExpect<'a, MapPoint>,
        ReadExpect<'a, HardDeadlineControl>,
        ReadExpect<'a, ForcedEliminationQueue>,
        WriteExpect<'a, AttackIntentQueue>,
        WriteExpect<'a, PendingDropQueue>,
        ReadExpect<'a, SpawnedDropIds>,
        WriteExpect<'a, MessageQueues>,
        ReadStorage<'a, MatchPlayerComp>,
        ReadStorage<'a, FixedEquipmentComp>,
        ReadStorage<'a, PositionComp>,
        ReadStorage<'a, DirectionComp>,
        WriteStorage<'a, HealthComp>,
        WriteStorage<'a, CombatComp>,
        WriteStorage<'a, ResourceInventoryComp>,
        WriteStorage<'a, MiningComp>,
        WriteStorage<'a, RoundStatsComp>,
        WriteStorage<'a, EliminationComp>,
        WriteStorage<'a, ExtractionComp>,
    );

    fn run(&mut self, data: Self::SystemData) {
        let (
            entities,
            context,
            authority,
            clients,
            chunks,
            registry,
            zone_point,
            hard_deadline,
            forced,
            mut intents,
            mut pending,
            spawned,
            mut queues,
            players,
            equipment,
            positions,
            directions,
            mut health,
            mut combat,
            mut inventories,
            mut mining,
            mut stats,
            mut eliminations,
            mut extractions,
        ) = data;
        let Some(now) = authority.monotonic_now() else {
            reject_all_unavailable(&context, &mut intents, &mut queues);
            return;
        };
        let Some(occurred_at) = authority.utc_now() else {
            reject_all_unavailable(&context, &mut intents, &mut queues);
            return;
        };

        process_forced_eliminations(TimeoutResolutionAccess {
            entities: &entities,
            context: &context,
            authority: &authority,
            now,
            occurred_at,
            forced: &forced,
            pending: &mut pending,
            spawned: &spawned,
            queues: &mut queues,
            players: &players,
            positions: &positions,
            health: &mut health,
            inventories: &mut inventories,
            mining: &mut mining,
            stats: &mut stats,
            eliminations: &mut eliminations,
            extractions: &extractions,
        });

        resolve_attacks(AttackResolutionAccess {
            entities: &entities,
            context: &context,
            authority: &authority,
            clients: &clients,
            chunks: &chunks,
            registry: &registry,
            now,
            occurred_at,
            intents: &mut intents,
            pending: &mut pending,
            spawned: &spawned,
            queues: &mut queues,
            players: &players,
            equipment: &equipment,
            positions: &positions,
            directions: &directions,
            health: &mut health,
            combat: &mut combat,
            inventories: &mut inventories,
            mining: &mut mining,
            stats: &mut stats,
            eliminations: &mut eliminations,
            extractions: &extractions,
        });

        process_extractions(ExtractionResolutionAccess {
            entities: &entities,
            context: &context,
            authority: &authority,
            clients: &clients,
            zone_point: *zone_point,
            queues: &mut queues,
            players: &players,
            equipment: &equipment,
            positions: &positions,
            health: &health,
            eliminations: &eliminations,
            extractions: &mut extractions,
            inventories: &mut inventories,
            mining: &mut mining,
        });
        if let Some(request) = hard_deadline.pending_request() {
            process_hard_deadline_eliminations(
                TimeoutResolutionAccess {
                    entities: &entities,
                    context: &context,
                    authority: &authority,
                    now: request.monotonic_deadline,
                    occurred_at: request.utc_deadline,
                    forced: &forced,
                    pending: &mut pending,
                    spawned: &spawned,
                    queues: &mut queues,
                    players: &players,
                    positions: &positions,
                    health: &mut health,
                    inventories: &mut inventories,
                    mining: &mut mining,
                    stats: &mut stats,
                    eliminations: &mut eliminations,
                    extractions: &extractions,
                },
                request,
                &hard_deadline,
            );
        }
    }
}
