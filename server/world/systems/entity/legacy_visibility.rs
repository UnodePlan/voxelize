use hashbrown::HashMap;
use specs::{Entity, ReadStorage};

use crate::{Bookkeeping, Clients, EntityOperation, EntityProtocol, KdTree, PositionComp, Vec3};

use super::visibility::DeletedEntityRecord;

#[allow(clippy::too_many_arguments)]
pub(super) fn project_legacy_entity_updates(
    clients: &Clients,
    kdtree: &KdTree,
    positions: &ReadStorage<'_, PositionComp>,
    radius: f32,
    changed: &HashMap<String, (String, String, bool)>,
    entity_positions: &HashMap<String, Vec3<f32>>,
    current: &HashMap<String, (String, Entity, String, bool)>,
    deleted: &[DeletedEntityRecord],
    bookkeeping: &mut Bookkeeping,
    client_updates: &mut HashMap<String, Vec<EntityProtocol>>,
) {
    let entity_to_client_id: HashMap<Entity, String> = clients
        .iter()
        .map(|(client_id, client)| (client.entity, client_id.clone()))
        .collect();

    for (entity_id, (etype, metadata, is_new)) in changed {
        let position = entity_positions
            .get(entity_id)
            .cloned()
            .unwrap_or(Vec3(0.0, 0.0, 0.0));
        for client_id in kdtree
            .players_within_radius(&position, radius)
            .into_iter()
            .filter_map(|entity| entity_to_client_id.get(entity))
        {
            let known = bookkeeping
                .client_known_entities
                .get(client_id)
                .is_some_and(|known| known.contains(entity_id));
            client_updates
                .entry(client_id.clone())
                .or_default()
                .push(EntityProtocol {
                    operation: if !known || *is_new {
                        EntityOperation::Create
                    } else {
                        EntityOperation::Update
                    },
                    id: entity_id.clone(),
                    r#type: etype.clone(),
                    metadata: Some(metadata.clone()),
                });
            bookkeeping
                .client_known_entities
                .entry(client_id.clone())
                .or_default()
                .insert(entity_id.clone());
        }
    }

    let all_client_ids: Vec<_> = clients.keys().cloned().collect();
    for entity in deleted {
        for client_id in &all_client_ids {
            let known = bookkeeping
                .client_known_entities
                .get(client_id)
                .is_some_and(|known| known.contains(&entity.id));
            if !known {
                continue;
            }
            client_updates
                .entry(client_id.clone())
                .or_default()
                .push(EntityProtocol {
                    operation: EntityOperation::Delete,
                    id: entity.id.clone(),
                    r#type: entity.etype.clone(),
                    metadata: Some(entity.metadata.clone()),
                });
            if let Some(known) = bookkeeping.client_known_entities.get_mut(client_id) {
                known.remove(&entity.id);
            }
        }
    }

    for (client_id, client) in clients.iter() {
        let Some(client_position) = positions.get(client.entity).map(|position| &position.0) else {
            continue;
        };
        let Some(known_entities) = bookkeeping.client_known_entities.get_mut(client_id) else {
            continue;
        };
        let entities_to_delete: Vec<_> = known_entities
            .iter()
            .filter(|entity_id| {
                if current
                    .get(*entity_id)
                    .is_some_and(|(etype, ..)| etype.starts_with("block::"))
                {
                    return false;
                }
                entity_positions.get(*entity_id).map_or(true, |position| {
                    let dx = position.0 - client_position.0;
                    let dy = position.1 - client_position.1;
                    let dz = position.2 - client_position.2;
                    dx * dx + dy * dy + dz * dz > radius * radius
                })
            })
            .cloned()
            .collect();

        for entity_id in entities_to_delete {
            if let Some((etype, _, metadata, _)) = current.get(&entity_id) {
                client_updates
                    .entry(client_id.clone())
                    .or_default()
                    .push(EntityProtocol {
                        operation: EntityOperation::Delete,
                        id: entity_id.clone(),
                        r#type: etype.clone(),
                        metadata: Some(metadata.clone()),
                    });
            }
            known_entities.remove(&entity_id);
        }
    }
}
