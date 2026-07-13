use std::time::Duration;

use serde_json::json;
use specs::{Entities, Join, WriteStorage};
use voxelize::{
    CurrentChunkComp, DoNotPersistComp, ETypeComp, EntityFlag, EntityIDs, IDComp, MetadataComp,
    PositionComp,
};

use super::{components::LootDropComp, runtime::GameplayRuntimeContext};
use crate::gameplay::{
    drop_queue::{EnqueueOutcome, PendingDropQueue, SpawnedDropIds},
    loot::LootDrop,
};

pub(super) struct DropSpawnAccess<'a, 'world> {
    pub entities: &'a Entities<'world>,
    pub context: &'a GameplayRuntimeContext,
    pub now: Duration,
    pub pending: &'a mut PendingDropQueue,
    pub spawned: &'a mut SpawnedDropIds,
    pub entity_ids: &'a mut EntityIDs,
    pub loots: &'a mut WriteStorage<'world, LootDropComp>,
    pub ids: &'a mut WriteStorage<'world, IDComp>,
    pub flags: &'a mut WriteStorage<'world, EntityFlag>,
    pub chunks: &'a mut WriteStorage<'world, CurrentChunkComp>,
    pub etypes: &'a mut WriteStorage<'world, ETypeComp>,
    pub metadatas: &'a mut WriteStorage<'world, MetadataComp>,
    pub positions: &'a mut WriteStorage<'world, PositionComp>,
    pub no_persist: &'a mut WriteStorage<'world, DoNotPersistComp>,
}

pub(super) fn spawn_pending_drops(access: DropSpawnAccess<'_, '_>) {
    let DropSpawnAccess {
        entities,
        context,
        now,
        pending,
        spawned,
        entity_ids,
        loots,
        ids,
        flags,
        chunks,
        etypes,
        metadatas,
        positions,
        no_persist,
    } = access;
    for drop in pending.drain_sorted() {
        if spawned.contains(drop.id()) {
            continue;
        }
        if merge_into_existing(
            entities,
            &drop,
            context.config.drop_merge_bucket_size,
            now,
            loots,
            metadatas,
        ) {
            spawned.insert(drop.id().clone());
            continue;
        }
        let spawned_entity = spawn_entity(
            drop.clone(),
            SpawnEntityAccess {
                entities,
                entity_ids,
                loots,
                ids,
                flags,
                chunks,
                etypes,
                metadatas,
                positions,
                no_persist,
            },
        );
        if spawned_entity {
            spawned.insert(drop.id().clone());
        } else {
            let outcome = pending.enqueue(drop);
            debug_assert_eq!(outcome, Ok(EnqueueOutcome::Inserted));
        }
    }
}

fn merge_into_existing(
    entities: &Entities<'_>,
    incoming: &LootDrop,
    bucket_size: f32,
    now: Duration,
    loots: &mut WriteStorage<'_, LootDropComp>,
    metadatas: &mut WriteStorage<'_, MetadataComp>,
) -> bool {
    let mut candidates = (entities, &*loots)
        .join()
        .map(|(entity, loot)| (loot.drop().id().clone(), entity))
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    for (_, entity) in candidates {
        let Some(loot) = loots.get_mut(entity) else {
            continue;
        };
        if loot.drop().can_merge(incoming, bucket_size, now).ok() != Some(true) {
            continue;
        }
        if loot
            .drop_mut()
            .merge(incoming.clone(), bucket_size, now)
            .ok()
            != Some(true)
        {
            continue;
        }
        if let Some(metadata) = metadatas.get_mut(entity) {
            set_loot_metadata(metadata, loot.drop());
        }
        return true;
    }
    false
}

struct SpawnEntityAccess<'a, 'world> {
    entities: &'a Entities<'world>,
    entity_ids: &'a mut EntityIDs,
    loots: &'a mut WriteStorage<'world, LootDropComp>,
    ids: &'a mut WriteStorage<'world, IDComp>,
    flags: &'a mut WriteStorage<'world, EntityFlag>,
    chunks: &'a mut WriteStorage<'world, CurrentChunkComp>,
    etypes: &'a mut WriteStorage<'world, ETypeComp>,
    metadatas: &'a mut WriteStorage<'world, MetadataComp>,
    positions: &'a mut WriteStorage<'world, PositionComp>,
    no_persist: &'a mut WriteStorage<'world, DoNotPersistComp>,
}

fn spawn_entity(drop: LootDrop, access: SpawnEntityAccess<'_, '_>) -> bool {
    let SpawnEntityAccess {
        entities,
        entity_ids,
        loots,
        ids,
        flags,
        chunks,
        etypes,
        metadatas,
        positions,
        no_persist,
    } = access;
    let id = drop.id().as_str().to_owned();
    if entity_ids.contains_key(&id) {
        return false;
    }
    let position = drop.position();
    let mut metadata = MetadataComp::new();
    set_loot_metadata(&mut metadata, &drop);
    let entity = entities.create();
    let inserted = ids.insert(entity, IDComp::new(&id)).is_ok()
        && flags.insert(entity, EntityFlag).is_ok()
        && chunks.insert(entity, CurrentChunkComp::default()).is_ok()
        && etypes
            .insert(entity, ETypeComp::new("extraction:loot", false))
            .is_ok()
        && metadatas.insert(entity, metadata).is_ok()
        && positions
            .insert(
                entity,
                PositionComp::new(position[0], position[1], position[2]),
            )
            .is_ok()
        && no_persist.insert(entity, DoNotPersistComp).is_ok()
        && loots.insert(entity, LootDropComp::new(drop)).is_ok();
    if !inserted {
        let _ = entities.delete(entity);
        return false;
    }
    entity_ids.insert(id, entity.id());
    true
}

pub(super) fn set_loot_metadata(metadata: &mut MetadataComp, drop: &LootDrop) {
    metadata.set_value(
        "loot",
        json!({
            "id": drop.id(),
            "contents": drop.contents(),
            "revision": drop.revision(),
        }),
    );
}
