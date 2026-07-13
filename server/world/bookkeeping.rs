use hashbrown::{HashMap, HashSet};
use specs::Entity;

use crate::Vec3;

#[derive(Default)]
pub struct Bookkeeping {
    // id -> (etype, entity, serialized_metadata, persisted)
    pub(crate) entities: HashMap<String, (String, Entity, String, bool)>,
    // Track entity positions for distance-based visibility
    // entity_id -> position
    pub(crate) entity_positions: HashMap<String, Vec3<f32>>,
    // Track which entities each client knows about
    // client_id -> set of entity_ids
    pub(crate) client_known_entities: HashMap<String, HashSet<String>>,
    // 可见性受限的 World 只向客户端同步半径内的其他玩家。
    pub(crate) client_known_peers: HashMap<String, HashSet<String>>,
}

impl Bookkeeping {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn remove_client(&mut self, client_id: &str) {
        self.client_known_entities.remove(client_id);
        self.client_known_peers.remove(client_id);
    }

    pub(crate) fn take_peer_viewers(&mut self, peer_id: &str) -> Vec<String> {
        let mut viewers = Vec::new();
        for (viewer_id, known) in &mut self.client_known_peers {
            if known.remove(peer_id) {
                viewers.push(viewer_id.clone());
            }
        }
        viewers.sort();
        viewers
    }
}
