use super::*;

impl World {
    pub(super) fn generate_client_init(
        &mut self,
        id: &str,
        entity: Entity,
    ) -> (Message, Vec<String>, Vec<String>) {
        let viewer_position = self
            .read_component::<PositionComp>()
            .get(entity)
            .map(|value| value.0.clone());
        let position = viewer_position
            .as_ref()
            .map(|value| [value.0, value.1, value.2])
            .filter(|value| value.iter().any(|coordinate| *coordinate != 0.0));
        let direction = self
            .read_component::<DirectionComp>()
            .get(entity)
            .map(|value| [value.0 .0, value.0 .1, value.0 .2])
            .filter(|value| value.iter().any(|coordinate| *coordinate != 0.0));
        let body = self.read_component::<RigidBodyComp>();
        let flying = body
            .get(entity)
            .map(|value| value.0.gravity_multiplier == 0.0 && value.0.aabb.width() > 0.0);
        let ghost = body.get(entity).map(|value| value.0.aabb.width() <= 0.0);
        let swimming = body.get(entity).map(|value| value.0.is_swimming);
        drop(body);

        self.generate_init_message(
            id,
            viewer_position,
            position,
            direction,
            flying,
            ghost,
            swimming,
        )
    }

    pub(super) fn replace_known_entities(&mut self, id: &str, entity_ids: Vec<String>) {
        let mut bookkeeping = self.write_resource::<Bookkeeping>();
        let known = bookkeeping
            .client_known_entities
            .entry(id.to_owned())
            .or_default();
        known.clear();
        known.extend(entity_ids);
    }

    pub(super) fn replace_known_peers(&mut self, id: &str, peer_ids: Vec<String>) {
        self.write_resource::<Bookkeeping>()
            .client_known_peers
            .insert(id.to_owned(), peer_ids.into_iter().collect());
    }

    pub(super) fn announce_client_join(&mut self, id: &str, mut visible_viewers: Vec<String>) {
        let bounded_visibility = super::visibility::bounded_visibility_radius(
            self.config().entity_visibility_policy,
            self.config().entity_visible_radius,
        )
        .is_some();
        if !bounded_visibility {
            self.broadcast(
                Message::new(&MessageType::Join).text(id).build(),
                ClientFilter::All,
            );
            return;
        }

        visible_viewers.sort();
        visible_viewers.dedup();
        {
            let mut bookkeeping = self.write_resource::<Bookkeeping>();
            for viewer_id in &visible_viewers {
                bookkeeping
                    .client_known_peers
                    .entry(viewer_id.clone())
                    .or_default()
                    .insert(id.to_owned());
            }
        }
        self.broadcast(
            Message::new(&MessageType::Join).text(id).build(),
            ClientFilter::Include(visible_viewers),
        );
    }

    pub(super) fn clear_client_visibility(&mut self, id: &str) -> ClientFilter {
        let bounded_visibility = super::visibility::bounded_visibility_radius(
            self.config().entity_visibility_policy,
            self.config().entity_visible_radius,
        )
        .is_some();
        let visible_viewers = if bounded_visibility {
            self.bookkeeping_mut().take_peer_viewers(id)
        } else {
            Vec::new()
        };
        self.bookkeeping_mut().remove_client(id);
        if bounded_visibility {
            ClientFilter::Include(visible_viewers)
        } else {
            ClientFilter::All
        }
    }
}
