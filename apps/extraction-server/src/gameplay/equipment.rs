use serde::Serialize;

use crate::contracts::EquipmentKey;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FixedEquipmentSnapshot {
    pub pickaxe: EquipmentKey,
    pub melee_weapon: EquipmentKey,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct FixedEquipment {
    pickaxe: EquipmentKey,
    melee_weapon: EquipmentKey,
}

impl FixedEquipment {
    pub(crate) const fn standard() -> Self {
        Self {
            pickaxe: EquipmentKey::BasicPickaxe,
            melee_weapon: EquipmentKey::BasicMeleeWeapon,
        }
    }

    pub(crate) const fn snapshot(self) -> FixedEquipmentSnapshot {
        FixedEquipmentSnapshot {
            pickaxe: self.pickaxe,
            melee_weapon: self.melee_weapon,
        }
    }

    #[cfg(feature = "engine")]
    pub(crate) const fn has_basic_pickaxe(self) -> bool {
        matches!(self.pickaxe, EquipmentKey::BasicPickaxe)
    }

    #[cfg(feature = "engine")]
    pub(crate) const fn has_basic_melee_weapon(self) -> bool {
        matches!(self.melee_weapon, EquipmentKey::BasicMeleeWeapon)
    }
}
