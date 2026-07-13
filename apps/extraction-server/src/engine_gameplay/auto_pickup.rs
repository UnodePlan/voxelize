use std::{collections::BTreeMap, time::Duration};

use specs::{Entities, Entity, Join, ReadStorage, WriteStorage};
use voxelize::{EntityIDs, MetadataComp, PositionComp};

use super::{
    authority::GameplayAuthority,
    components::{
        EliminationComp, ExtractionComp, HealthComp, LootDropComp, MatchPlayerComp,
        ResourceInventoryComp, RoundStatsComp,
    },
    drop_spawn::set_loot_metadata,
};
use crate::gameplay::transactions::{ordered_pickup_candidates, PickupCandidate};

pub(super) struct AutoPickupAccess<'a, 'world> {
    pub entities: &'a Entities<'world>,
    pub authority: &'a GameplayAuthority,
    pub now: Duration,
    pub radius: f32,
    pub entity_ids: &'a mut EntityIDs,
    pub players: &'a ReadStorage<'world, MatchPlayerComp>,
    pub health: &'a ReadStorage<'world, HealthComp>,
    pub eliminations: &'a WriteStorage<'world, EliminationComp>,
    pub extractions: &'a WriteStorage<'world, ExtractionComp>,
    pub inventories: &'a mut WriteStorage<'world, ResourceInventoryComp>,
    pub stats: &'a mut WriteStorage<'world, RoundStatsComp>,
    pub positions: &'a WriteStorage<'world, PositionComp>,
    pub loots: &'a mut WriteStorage<'world, LootDropComp>,
    pub metadatas: &'a mut WriteStorage<'world, MetadataComp>,
    pub dirty_players: &'a mut BTreeMap<String, Entity>,
}

pub(super) fn auto_pickup(access: AutoPickupAccess<'_, '_>) {
    let AutoPickupAccess {
        entities,
        authority,
        now,
        radius,
        entity_ids,
        players,
        health,
        eliminations,
        extractions,
        inventories,
        stats,
        positions,
        loots,
        metadatas,
        dirty_players,
    } = access;
    let mut candidates = (
        entities,
        players,
        positions,
        health,
        eliminations,
        extractions,
    )
        .join()
        .filter_map(
            |(entity, player, position, health, elimination, extraction)| {
                let client_id = player.public_player_id().to_string();
                (health.state().is_alive()
                    && elimination.record().is_none()
                    && extraction.record().is_none()
                    && authority.allows(&client_id, player.account_id()))
                .then_some((
                    entity,
                    client_id,
                    PickupCandidate {
                        seat_id: player.seat_id(),
                        account_id: player.account_id(),
                        position: position.0.to_arr(),
                    },
                ))
            },
        )
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| candidate.2.seat_id);
    let domain_candidates = candidates
        .iter()
        .map(|candidate| candidate.2)
        .collect::<Vec<_>>();

    let mut drops = (entities, &*loots)
        .join()
        .map(|(entity, loot)| (loot.drop().id().clone(), entity))
        .collect::<Vec<_>>();
    drops.sort_by(|left, right| left.0.cmp(&right.0));

    for (drop_id, drop_entity) in drops {
        let order = loots.get(drop_entity).and_then(|loot| {
            ordered_pickup_candidates(loot.drop(), &domain_candidates, radius, now).ok()
        });
        let Some(order) = order else {
            continue;
        };
        for candidate_index in order {
            let (player_entity, client_id, _) = &candidates[candidate_index];
            let Some(mut inventory_candidate) = inventories
                .get(*player_entity)
                .map(|inventory| inventory.inventory().clone())
            else {
                continue;
            };
            let Some(mut loot_candidate) = loots.get(drop_entity).map(|loot| loot.drop().clone())
            else {
                break;
            };
            let Some(mut stats_candidate) =
                stats.get(*player_entity).map(|stats| stats.stats().clone())
            else {
                continue;
            };
            let Ok(accepted) = loot_candidate.transfer_into_with_receipt(&mut inventory_candidate)
            else {
                continue;
            };
            if !accepted.is_empty() && stats_candidate.record_picked_up(accepted).is_err() {
                continue;
            }
            if !accepted.is_empty() {
                *inventories.get_mut(*player_entity).unwrap().inventory_mut() = inventory_candidate;
                *loots.get_mut(drop_entity).unwrap().drop_mut() = loot_candidate;
                *stats.get_mut(*player_entity).unwrap().stats_mut() = stats_candidate;
                dirty_players.insert(client_id.clone(), *player_entity);
            }
            if loots
                .get(drop_entity)
                .is_some_and(|loot| loot.drop().is_empty())
            {
                break;
            }
        }

        let Some(loot) = loots.get(drop_entity) else {
            continue;
        };
        if loot.drop().is_empty() {
            entity_ids.remove(drop_id.as_str());
            let _ = entities.delete(drop_entity);
        } else if let Some(metadata) = metadatas.get_mut(drop_entity) {
            set_loot_metadata(metadata, loot.drop());
        }
    }
}
