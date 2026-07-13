use std::time::Duration;

use specs::{Entities, ReadStorage, WriteStorage};
use voxelize::{Chunks, Clients, DirectionComp, MessageQueues, PositionComp};

use super::{
    authority::GameplayAuthority,
    components::{EliminationComp, FixedEquipmentComp, MatchPlayerComp, MiningComp},
    intents::MiningIntentQueue,
    mining_dirty::MiningDirtyPlayers,
    mining_intent_actions::{
        apply_cancel, apply_maintain, apply_start, queue_intent_error, reject_intent,
    },
    runtime::GameplayRuntimeContext,
};
use crate::{
    contracts::{ErrorCode, MiningIdleReason, MiningPayload},
    gameplay::harvest::HarvestedVoxelSet,
};

pub(super) struct MiningIntentAccess<'a, 'world> {
    pub entities: &'a Entities<'world>,
    pub context: &'a GameplayRuntimeContext,
    pub authority: &'a GameplayAuthority,
    pub now: Duration,
    pub clients: &'a Clients,
    pub intents: &'a mut MiningIntentQueue,
    pub queues: &'a mut MessageQueues,
    pub chunks: &'a Chunks,
    pub harvested: &'a HarvestedVoxelSet,
    pub players: &'a ReadStorage<'world, MatchPlayerComp>,
    pub equipment: &'a ReadStorage<'world, FixedEquipmentComp>,
    pub positions: &'a ReadStorage<'world, PositionComp>,
    pub directions: &'a ReadStorage<'world, DirectionComp>,
    pub eliminations: &'a ReadStorage<'world, EliminationComp>,
    pub mining: &'a mut WriteStorage<'world, MiningComp>,
    pub dirty: &'a mut MiningDirtyPlayers,
}

pub(super) fn process_mining_intents(mut access: MiningIntentAccess<'_, '_>) {
    let intents = access.intents.drain().collect::<Vec<_>>();
    for intent in intents {
        let Some(player) = access.players.get(intent.entity) else {
            queue_intent_error(
                &mut access,
                &intent.client_id,
                intent.request_id,
                ErrorCode::GameInvalidState,
                false,
            );
            continue;
        };
        let authorized = access.entities.is_alive(intent.entity)
            && player.public_player_id().to_string() == intent.client_id
            && access.authority.allows_entity(
                access.clients,
                intent.entity,
                &intent.client_id,
                player.account_id(),
            )
            && access
                .eliminations
                .get(intent.entity)
                .is_some_and(|state| state.record().is_none());
        if !authorized {
            reject_intent(
                &mut access,
                intent.entity,
                &intent.client_id,
                intent.request_id,
                intent.sequence,
                MiningIdleReason::Disconnected,
                (ErrorCode::GameInvalidState, false),
            );
            continue;
        }

        match intent.payload {
            MiningPayload::Start { voxel } => apply_start(&mut access, intent, voxel.into()),
            MiningPayload::Maintain {} => apply_maintain(&mut access, intent),
            MiningPayload::Cancel {} => apply_cancel(&mut access, intent),
        }
    }
}
