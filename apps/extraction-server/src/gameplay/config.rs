use std::time::Duration;

use crate::{contracts::ResourceKey, match_world::RESOURCE_BACKPACK_SLOTS};

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
    pub mining_reach: f32,
    pub mining_eye_offset: f32,
    pub mining_maintain_grace: Duration,
    pub mining_sync_interval: Duration,
    pub dirt_mining_duration: Duration,
    pub gold_mining_duration: Duration,
    pub diamond_mining_duration: Duration,
    pub extraction_radius: f32,
    pub extraction_half_height: f32,
    pub extraction_hold_duration: Duration,
}

impl GameplayConfig {
    pub(crate) fn resolve(gameplay_version: &str, config_version: &str) -> Option<&'static Self> {
        match (gameplay_version, config_version) {
            ("pvp-mvp-v1", "balance-v1") => Some(&GAMEPLAY_V1),
            _ => None,
        }
    }

    pub(crate) const fn mining_duration(self, resource: ResourceKey) -> Duration {
        match resource {
            ResourceKey::Dirt => self.dirt_mining_duration,
            ResourceKey::Gold => self.gold_mining_duration,
            ResourceKey::Diamond => self.diamond_mining_duration,
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
    mining_reach: 4.5,
    // Voxelize controls 上报的 PositionComp 已是服务端接受的相机/眼睛位置，不能重复叠加眼高。
    mining_eye_offset: 0.0,
    mining_maintain_grace: Duration::from_millis(350),
    mining_sync_interval: Duration::from_millis(50),
    dirt_mining_duration: Duration::from_millis(500),
    gold_mining_duration: Duration::from_millis(1_500),
    diamond_mining_duration: Duration::from_millis(3_000),
    extraction_radius: 4.0,
    extraction_half_height: 3.0,
    extraction_hold_duration: Duration::from_secs(8),
};
