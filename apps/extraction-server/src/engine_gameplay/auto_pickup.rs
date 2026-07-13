use std::{collections::BTreeMap, time::Duration};

use specs::{Entities, Entity, Join, ReadStorage, WriteStorage};
use voxelize::{EntityIDs, MetadataComp, PositionComp};

use super::{
    authority::GameplayAuthority,
    components::{LootDropComp, MatchPlayerComp, ResourceInventoryComp},
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
    pub inventories: &'a mut WriteStorage<'world, ResourceInventoryComp>,
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
        inventories,
        positions,
        loots,
        metadatas,
        dirty_players,
    } = access;
    let mut candidates = (entities, players, positions)
        .join()
        .filter_map(|(entity, player, position)| {
            let client_id = player.public_player_id().to_string();
            authority
                .allows(&client_id, player.account_id())
                .then_some((
                    entity,
                    client_id,
                    PickupCandidate {
                        seat_id: player.seat_id(),
                        account_id: player.account_id(),
                        position: position.0.to_arr(),
                    },
                ))
        })
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
            let Some(inventory) = inventories.get_mut(*player_entity) else {
                continue;
            };
            let Some(loot) = loots.get_mut(drop_entity) else {
                break;
            };
            let Ok(accepted) = loot.drop_mut().transfer_into(inventory.inventory_mut()) else {
                continue;
            };
            if accepted > 0 {
                dirty_players.insert(client_id.clone(), *player_entity);
            }
            if loot.drop().is_empty() {
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
