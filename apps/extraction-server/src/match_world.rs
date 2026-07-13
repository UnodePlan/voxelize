use uuid::Uuid;

use crate::contracts::EquipmentKey;

pub const MATCH_PLAYER_CAPACITY: usize = 10;
pub const RESOURCE_BACKPACK_SLOTS: usize = 12;
pub const MAX_HEALTH_HALF_HEARTS: u8 = 20;
pub const PLAYER_BODY_WIDTH: f32 = 0.8;
pub const PLAYER_BODY_HEIGHT: f32 = 1.8;
pub const PLAYER_BODY_DEPTH: f32 = 0.8;
pub const PLAYER_EYE_HEIGHT: f32 = 1.62;
pub const PLAYER_EYE_OFFSET_FROM_CENTER: f32 = PLAYER_EYE_HEIGHT - PLAYER_BODY_HEIGHT / 2.0;
pub const ENGINE_MIN_CHUNK: [i32; 2] = [-10, -10];
pub const ENGINE_MAX_CHUNK: [i32; 2] = [9, 9];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayableBounds {
    pub min_inclusive: f32,
    pub max_exclusive: f32,
}

impl PlayableBounds {
    pub const EXTRACTION: Self = Self {
        min_inclusive: -150.0,
        max_exclusive: 150.0,
    };

    pub fn contains_xz(self, x: f32, z: f32) -> bool {
        x >= self.min_inclusive
            && x < self.max_exclusive
            && z >= self.min_inclusive
            && z < self.max_exclusive
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixedMatchLoadout {
    pub pickaxe: EquipmentKey,
    pub melee_weapon: EquipmentKey,
    pub resource_backpack_slots: usize,
    pub max_health_half_hearts: u8,
}

impl Default for FixedMatchLoadout {
    fn default() -> Self {
        Self {
            pickaxe: EquipmentKey::BasicPickaxe,
            melee_weapon: EquipmentKey::BasicMeleeWeapon,
            resource_backpack_slots: RESOURCE_BACKPACK_SLOTS,
            max_health_half_hearts: MAX_HEALTH_HALF_HEARTS,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MatchWorldMetadata {
    pub match_id: Uuid,
    pub seed: u64,
    pub engine_seed: u32,
    pub generation_version: String,
    pub gameplay_version: String,
    pub config_version: String,
    pub catalog_version: u32,
    pub loadout: FixedMatchLoadout,
}

/// V1 generation folds both halves so every persisted seed bit affects the engine seed.
pub const fn engine_seed_v1(seed: u64) -> u32 {
    (seed as u32) ^ ((seed >> 32) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gameplay_bounds_are_exactly_three_hundred_blocks() {
        let bounds = PlayableBounds::EXTRACTION;
        assert_eq!(bounds.max_exclusive - bounds.min_inclusive, 300.0);
        assert!(bounds.contains_xz(-150.0, -150.0));
        assert!(bounds.contains_xz(149.999, 149.999));
        assert!(!bounds.contains_xz(150.0, 0.0));
        assert!(!bounds.contains_xz(0.0, -150.001));
    }

    #[test]
    fn engine_seed_mapping_is_stable_and_uses_both_halves() {
        assert_eq!(engine_seed_v1(0x1122_3344_5566_7788), 0x4444_44cc);
        assert_ne!(engine_seed_v1(1), engine_seed_v1(2_u64 << 32));
    }

    #[test]
    fn fixed_loadout_never_depends_on_warehouse_contents() {
        assert_eq!(
            FixedMatchLoadout::default(),
            FixedMatchLoadout {
                pickaxe: EquipmentKey::BasicPickaxe,
                melee_weapon: EquipmentKey::BasicMeleeWeapon,
                resource_backpack_slots: 12,
                max_health_half_hearts: 20,
            }
        );
    }
}
