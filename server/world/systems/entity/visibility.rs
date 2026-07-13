use hashbrown::HashSet;

use crate::{world::visibility::is_position_visible, EntityOperation, EntityProtocol, Vec3};

pub(super) struct EntityVisibilityRecord {
    pub id: String,
    pub etype: String,
    pub metadata: String,
    pub position: Option<Vec3<f32>>,
    pub changed: bool,
    pub is_new: bool,
}

pub(super) struct DeletedEntityRecord {
    pub id: String,
    pub etype: String,
    pub metadata: String,
}

pub(super) struct EntityVisibilityProjection {
    pub updates: Vec<EntityProtocol>,
    pub visible: HashSet<String>,
}

pub(super) fn project_entity_visibility(
    viewer: &Vec3<f32>,
    known: &HashSet<String>,
    current: &[EntityVisibilityRecord],
    deleted: &[DeletedEntityRecord],
    radius: f32,
) -> EntityVisibilityProjection {
    let mut updates = Vec::new();
    let mut visible = HashSet::new();

    for entity in current {
        if !entity
            .position
            .as_ref()
            .is_some_and(|position| is_position_visible(viewer, position, radius))
        {
            continue;
        }
        let entering = !known.contains(&entity.id);
        if entering || entity.is_new || entity.changed {
            updates.push(EntityProtocol {
                operation: if entering || entity.is_new {
                    EntityOperation::Create
                } else {
                    EntityOperation::Update
                },
                id: entity.id.clone(),
                r#type: entity.etype.clone(),
                metadata: Some(entity.metadata.clone()),
            });
        }
        visible.insert(entity.id.clone());
    }

    let mut leaving: Vec<_> = known.difference(&visible).cloned().collect();
    leaving.sort();
    for entity_id in leaving {
        let record = current
            .iter()
            .find(|entity| entity.id == entity_id)
            .map(|entity| entity.etype.as_str())
            .or_else(|| {
                deleted
                    .iter()
                    .find(|entity| entity.id == entity_id)
                    .map(|entity| entity.etype.as_str())
            });
        let Some(etype) = record else {
            continue;
        };
        updates.push(EntityProtocol {
            operation: EntityOperation::Delete,
            id: entity_id,
            r#type: etype.to_owned(),
            // 离开可见半径时不能把边界外的最新坐标附在 DELETE 上。
            metadata: None,
        });
    }

    EntityVisibilityProjection { updates, visible }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(id: &str, x: f32, changed: bool) -> EntityVisibilityRecord {
        EntityVisibilityRecord {
            id: id.to_owned(),
            etype: "loot".to_owned(),
            metadata: format!(r#"{{"x":{x}}}"#),
            position: Some(Vec3(x, 0.0, 0.0)),
            changed,
            is_new: false,
        }
    }

    #[test]
    fn projection_creates_entering_and_deletes_leaving_without_far_update() {
        let known = HashSet::from(["far".to_owned(), "missing-position".to_owned()]);
        let mut missing_position = entity("missing-position", 0.0, true);
        missing_position.position = None;
        let current = vec![
            entity("near", 10.0, false),
            entity("far", 100.0, true),
            missing_position,
        ];

        let projection =
            project_entity_visibility(&Vec3(0.0, 0.0, 0.0), &known, &current, &[], 96.0);

        assert_eq!(projection.updates.len(), 3);
        assert_eq!(projection.updates[0].id, "near");
        assert_eq!(projection.updates[0].operation, EntityOperation::Create);
        assert_eq!(projection.updates[1].id, "far");
        assert_eq!(projection.updates[1].operation, EntityOperation::Delete);
        assert_eq!(projection.updates[1].metadata, None);
        assert_eq!(projection.updates[2].id, "missing-position");
        assert_eq!(projection.updates[2].operation, EntityOperation::Delete);
    }
}
