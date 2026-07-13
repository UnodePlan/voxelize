use hashbrown::HashSet;
use specs::{Join, ReadExpect, ReadStorage, System, WriteExpect, WriteStorage};

use crate::{
    world::visibility::{bounded_visibility_radius, is_position_visible},
    Bookkeeping, ClientFilter, ClientFlag, Clients, IDComp, Message, MessageQueues, MessageType,
    MetadataComp, NameComp, PeerProtocol, PositionComp, Vec3, WorldConfig,
};

pub struct PeersSendingSystem;

impl<'a> System<'a> for PeersSendingSystem {
    type SystemData = (
        ReadExpect<'a, Clients>,
        ReadExpect<'a, WorldConfig>,
        WriteExpect<'a, MessageQueues>,
        WriteExpect<'a, Bookkeeping>,
        ReadStorage<'a, ClientFlag>,
        ReadStorage<'a, IDComp>,
        ReadStorage<'a, NameComp>,
        ReadStorage<'a, PositionComp>,
        WriteStorage<'a, MetadataComp>,
    );

    fn run(&mut self, data: Self::SystemData) {
        let (
            clients,
            config,
            mut queue,
            mut bookkeeping,
            flags,
            ids,
            names,
            positions,
            mut metadatas,
        ) = data;

        let mut peers = Vec::new();
        for (id, name, position, metadata, _) in
            (&ids, &names, &positions, &mut metadatas, &flags).join()
        {
            let (json_str, updated) = metadata.to_cached_str();
            if updated {
                metadata.reset();
            }
            peers.push(PeerSnapshot {
                id: id.0.to_owned(),
                username: name.0.to_owned(),
                metadata: json_str,
                position: position.0.clone(),
                updated,
            });
        }
        peers.sort_by(|left, right| left.id.cmp(&right.id));

        let Some(radius) = bounded_visibility_radius(
            config.entity_visibility_policy,
            config.entity_visible_radius,
        ) else {
            let updates: Vec<_> = peers
                .into_iter()
                .filter(|peer| peer.updated)
                .map(PeerSnapshot::into_protocol)
                .collect();
            if !updates.is_empty() {
                queue.push((
                    Message::new(&MessageType::Peer).peers(&updates).build(),
                    ClientFilter::All,
                ));
            }
            return;
        };

        let mut viewer_ids: Vec<_> = clients.keys().cloned().collect();
        viewer_ids.sort();
        for viewer_id in viewer_ids {
            let viewer_position = clients
                .get(&viewer_id)
                .and_then(|client| positions.get(client.entity))
                .map(|position| position.0.clone())
                .unwrap_or(Vec3(f32::NAN, f32::NAN, f32::NAN));
            let known = bookkeeping
                .client_known_peers
                .get(&viewer_id)
                .cloned()
                .unwrap_or_default();
            let projection =
                project_visibility(&viewer_id, &viewer_position, &peers, &known, radius);
            bookkeeping
                .client_known_peers
                .insert(viewer_id.clone(), projection.visible);

            for peer_id in projection.joins {
                queue.push((
                    Message::new(&MessageType::Join).text(&peer_id).build(),
                    ClientFilter::Direct(viewer_id.clone()),
                ));
            }
            if !projection.updates.is_empty() {
                queue.push((
                    Message::new(&MessageType::Peer)
                        .peers(&projection.updates)
                        .build(),
                    ClientFilter::Direct(viewer_id.clone()),
                ));
            }
            for peer_id in projection.leaves {
                queue.push((
                    Message::new(&MessageType::Leave).text(&peer_id).build(),
                    ClientFilter::Direct(viewer_id.clone()),
                ));
            }
        }
    }
}

struct PeerSnapshot {
    id: String,
    username: String,
    metadata: String,
    position: Vec3<f32>,
    updated: bool,
}

impl PeerSnapshot {
    fn into_protocol(self) -> PeerProtocol {
        PeerProtocol {
            id: self.id,
            username: self.username,
            metadata: self.metadata,
        }
    }

    fn to_protocol(&self) -> PeerProtocol {
        PeerProtocol {
            id: self.id.clone(),
            username: self.username.clone(),
            metadata: self.metadata.clone(),
        }
    }
}

struct PeerProjection {
    joins: Vec<String>,
    leaves: Vec<String>,
    updates: Vec<PeerProtocol>,
    visible: HashSet<String>,
}

fn project_visibility(
    viewer_id: &str,
    viewer_position: &Vec3<f32>,
    peers: &[PeerSnapshot],
    known: &HashSet<String>,
    radius: f32,
) -> PeerProjection {
    let mut joins = Vec::new();
    let mut updates = Vec::new();
    let mut visible = HashSet::new();

    for peer in peers {
        if peer.id == viewer_id {
            // 即使服务端回滚后位置不再变化，也要持续纠正客户端的本地预测。
            updates.push(peer.to_protocol());
            continue;
        }
        if !is_position_visible(viewer_position, &peer.position, radius) {
            continue;
        }
        let entering = !known.contains(&peer.id);
        if entering {
            joins.push(peer.id.clone());
        }
        if entering || peer.updated {
            updates.push(peer.to_protocol());
        }
        visible.insert(peer.id.clone());
    }

    let mut leaves: Vec<_> = known.difference(&visible).cloned().collect();
    joins.sort();
    leaves.sort();
    PeerProjection {
        joins,
        leaves,
        updates,
        visible,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peer(id: &str, x: f32, updated: bool) -> PeerSnapshot {
        PeerSnapshot {
            id: id.to_owned(),
            username: id.to_owned(),
            metadata: format!(r#"{{"position":[{x},0,0]}}"#),
            position: Vec3(x, 0.0, 0.0),
            updated,
        }
    }

    #[test]
    fn bounded_projection_enters_updates_and_leaves_without_far_metadata() {
        let viewer = Vec3(0.0, 0.0, 0.0);
        let known = HashSet::from(["far".to_owned()]);
        let peers = vec![
            peer("self", 0.0, true),
            peer("near", 10.0, false),
            peer("far", 100.0, true),
        ];

        let projection = project_visibility("self", &viewer, &peers, &known, 96.0);

        assert_eq!(projection.joins, vec!["near"]);
        assert_eq!(projection.leaves, vec!["far"]);
        assert_eq!(
            projection
                .updates
                .iter()
                .map(|peer| peer.id.as_str())
                .collect::<Vec<_>>(),
            vec!["self", "near"]
        );
        assert!(!projection.updates.iter().any(|peer| peer.id == "far"));
    }

    #[test]
    fn bounded_projection_repeats_unchanged_self_for_prediction_correction() {
        let viewer = Vec3(149.5, 2.0, 0.0);
        let peers = vec![peer("self", 149.5, false)];

        let projection = project_visibility("self", &viewer, &peers, &HashSet::new(), 96.0);

        assert_eq!(projection.updates.len(), 1);
        assert_eq!(projection.updates[0].id, "self");
        assert!(projection.joins.is_empty());
        assert!(projection.leaves.is_empty());
    }
}
