use std::time::Duration;

use crate::match_world::RESOURCE_BACKPACK_SLOTS;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct GameplayConfig {
    pub gameplay_version: &'static str,
    pub config_version: &'static str,
    pub inventory_slots: usize,
    pub max_stack: u32,
    pub pickup_radius: f32,
    pub manual_drop_distance: f32,
    pub manual_drop_height: f32,
    pub manual_drop_exclusion: Duration,
    pub drop_merge_bucket_size: f32,
    pub intent_queue_capacity: usize,
}

impl GameplayConfig {
    pub(crate) fn resolve(gameplay_version: &str, config_version: &str) -> Option<&'static Self> {
        match (gameplay_version, config_version) {
            ("pvp-mvp-v1", "balance-v1") => Some(&GAMEPLAY_V1),
            _ => None,
        }
    }
}

pub(crate) const GAMEPLAY_V1: GameplayConfig = GameplayConfig {
    gameplay_version: "pvp-mvp-v1",
    config_version: "balance-v1",
    inventory_slots: RESOURCE_BACKPACK_SLOTS,
    max_stack: 64,
    pickup_radius: 2.0,
    manual_drop_distance: 1.25,
    manual_drop_height: 0.35,
    manual_drop_exclusion: Duration::from_secs(2),
    drop_merge_bucket_size: 2.0,
    intent_queue_capacity: 64,
};
