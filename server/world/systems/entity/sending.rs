use hashbrown::{HashMap, HashSet};
use specs::{
    Entities, Entity, Join, LendJoin, ReadExpect, ReadStorage, System, WriteExpect, WriteStorage,
};

use crate::{
    world::visibility::bounded_visibility_radius, BackgroundEntitiesSaver, Bookkeeping,
    ClientFilter, Clients, DoNotPersistComp, ETypeComp, EntityFlag, EntityIDs, EntityProtocol,
    IDComp, InteractorComp, KdTree, Message, MessageQueues, MessageType, MetadataComp, Physics,
    PositionComp, Vec3, VoxelComp, WorldConfig,
};

use super::legacy_visibility::project_legacy_entity_updates;
use super::visibility::{project_entity_visibility, DeletedEntityRecord, EntityVisibilityRecord};

#[derive(Default)]
pub struct EntitiesSendingSystem {
    updated_entities_buffer: Vec<(String, Entity)>,
    entity_updates_buffer: Vec<EntityProtocol>,
    new_entity_ids_buffer: HashSet<String>,
}

impl<'a> System<'a> for EntitiesSendingSystem {
    type SystemData = (
        Entities<'a>,
        ReadExpect<'a, BackgroundEntitiesSaver>,
        ReadExpect<'a, KdTree>,
        ReadExpect<'a, Clients>,
        ReadExpect<'a, WorldConfig>,
        WriteExpect<'a, MessageQueues>,
        WriteExpect<'a, Bookkeeping>,
        WriteExpect<'a, Physics>,
        WriteExpect<'a, EntityIDs>,
        ReadStorage<'a, EntityFlag>,
        ReadStorage<'a, IDComp>,
        ReadStorage<'a, ETypeComp>,
        ReadStorage<'a, InteractorComp>,
        ReadStorage<'a, DoNotPersistComp>,
        ReadStorage<'a, PositionComp>,
        ReadStorage<'a, VoxelComp>,
        WriteStorage<'a, MetadataComp>,
    );

    fn run(&mut self, data: Self::SystemData) {
        let (
            entities,
            bg_saver,
            kdtree,
            clients,
            config,
            mut queue,
            mut bookkeeping,
            mut physics,
            mut entity_ids,
            flags,
            ids,
            etypes,
            interactors,
            do_not_persist,
            positions,
            voxels,
            mut metadatas,
        ) = data;

        self.updated_entities_buffer.clear();
        self.entity_updates_buffer.clear();
        self.new_entity_ids_buffer.clear();

        let entity_visible_radius = config.entity_visible_radius;
        let bounded_radius =
            bounded_visibility_radius(config.entity_visibility_policy, entity_visible_radius);

        let mut new_entity_handlers = HashMap::new();

        for (ent, interactor) in (&entities, &interactors).join() {
            new_entity_handlers.insert(
                ent,
                (
                    interactor.collider_handle().clone(),
                    interactor.body_handle().clone(),
                ),
            );
        }

        let mut updated_ids: HashSet<&String> = HashSet::new();

        for (id, ent, _) in (&ids, &entities, &flags).join() {
            updated_ids.insert(&id.0);
            self.updated_entities_buffer.push((id.0.to_owned(), ent));
        }

        let old_entities = std::mem::take(&mut bookkeeping.entities);
        let old_ids: HashSet<&String> = old_entities.keys().collect();
        let _old_entity_positions = std::mem::take(&mut bookkeeping.entity_positions);

        let old_entity_handlers = std::mem::take(&mut physics.entity_to_handlers);

        let mut deleted_entities = Vec::new();

        for (id, (etype, ent, metadata, persisted)) in old_entities.iter() {
            if updated_ids.contains(id) {
                continue;
            }

            if *persisted {
                bg_saver.remove(id);
            }
            entity_ids.remove(id);

            if let Some((collider_handle, body_handle)) = old_entity_handlers.get(ent) {
                physics.unregister(body_handle, collider_handle);
            }

            deleted_entities.push(DeletedEntityRecord {
                id: id.clone(),
                etype: etype.clone(),
                metadata: metadata.clone(),
            });
        }

        physics.entity_to_handlers = new_entity_handlers;

        for (id, _) in &self.updated_entities_buffer {
            if !old_ids.contains(id) {
                self.new_entity_ids_buffer.insert(id.to_owned());
            }
        }

        let mut new_bookkeeping_records = HashMap::new();
        let mut entity_positions: HashMap<String, Vec3<f32>> = HashMap::new();
        let mut entity_metadata_map: HashMap<String, (String, String, bool)> = HashMap::new();
        let mut visibility_records = Vec::new();

        for (ent, id, metadata, etype, _, do_not_persist, position, voxel) in (
            &entities,
            &ids,
            &mut metadatas,
            &etypes,
            &flags,
            do_not_persist.maybe(),
            positions.maybe(),
            voxels.maybe(),
        )
            .join()
        {
            if metadata.is_empty() {
                continue;
            }

            let persisted = do_not_persist.is_none();

            let spatial_position = position
                .map(|p| p.0.clone())
                .or_else(|| voxel.map(|v| Vec3(v.0 .0 as f32, v.0 .1 as f32, v.0 .2 as f32)));
            let pos = spatial_position.clone().unwrap_or(Vec3(0.0, 0.0, 0.0));
            entity_positions.insert(id.0.clone(), pos.clone());

            let is_new = self.new_entity_ids_buffer.contains(&id.0);
            let (json_str, updated) = metadata.to_cached_str();

            if is_new || updated {
                entity_metadata_map
                    .insert(id.0.clone(), (etype.0.clone(), json_str.clone(), is_new));
            }

            visibility_records.push(EntityVisibilityRecord {
                id: id.0.clone(),
                etype: etype.0.clone(),
                metadata: json_str.clone(),
                position: spatial_position,
                changed: updated,
                is_new,
            });

            new_bookkeeping_records.insert(
                id.0.to_owned(),
                (etype.0.to_owned(), ent, json_str, persisted),
            );
        }

        let mut client_updates: HashMap<String, Vec<EntityProtocol>> = HashMap::new();
        if let Some(radius) = bounded_radius {
            visibility_records.sort_by(|left, right| left.id.cmp(&right.id));
            let mut client_ids: Vec<_> = clients.keys().cloned().collect();
            client_ids.sort();
            for client_id in client_ids {
                let client_position = clients
                    .get(&client_id)
                    .and_then(|client| positions.get(client.entity))
                    .map(|position| position.0.clone())
                    .unwrap_or(Vec3(f32::NAN, f32::NAN, f32::NAN));
                let known = bookkeeping
                    .client_known_entities
                    .get(&client_id)
                    .cloned()
                    .unwrap_or_default();
                let projection = project_entity_visibility(
                    &client_position,
                    &known,
                    &visibility_records,
                    &deleted_entities,
                    radius,
                );
                bookkeeping
                    .client_known_entities
                    .insert(client_id.clone(), projection.visible);
                client_updates.insert(client_id, projection.updates);
            }
        } else {
            project_legacy_entity_updates(
                &clients,
                &kdtree,
                &positions,
                entity_visible_radius,
                &entity_metadata_map,
                &entity_positions,
                &new_bookkeeping_records,
                &deleted_entities,
                &mut bookkeeping,
                &mut client_updates,
            );
        }

        bookkeeping.entities = new_bookkeeping_records;
        bookkeeping.entity_positions = entity_positions;

        for (client_id, updates) in client_updates {
            if !updates.is_empty() {
                queue.push((
                    Message::new(&MessageType::Entity)
                        .entities(&updates)
                        .build(),
                    ClientFilter::Direct(client_id),
                ));
            }
        }
    }
}
