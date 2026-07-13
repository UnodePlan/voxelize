use std::collections::BTreeMap;

use specs::Entity;

#[derive(Clone, Copy)]
pub(super) struct DirtyMiningPlayer {
    pub entity: Entity,
    pub inventory: bool,
}

#[derive(Default)]
pub(super) struct MiningDirtyPlayers(BTreeMap<String, DirtyMiningPlayer>);

impl MiningDirtyPlayers {
    pub(super) fn mark(&mut self, client_id: &str, entity: Entity, inventory: bool) {
        self.0
            .entry(client_id.to_owned())
            .and_modify(|dirty| dirty.inventory |= inventory)
            .or_insert(DirtyMiningPlayer { entity, inventory });
    }

    pub(super) fn into_inner(self) -> BTreeMap<String, DirtyMiningPlayer> {
        self.0
    }
}
