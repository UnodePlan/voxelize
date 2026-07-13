use specs::{Component, DenseVecStorage, VecStorage};
use uuid::Uuid;

use crate::{
    gameplay::{
        equipment::{FixedEquipment, FixedEquipmentSnapshot},
        inventory::{InventorySnapshot, MatchInventory},
        loot::LootDrop,
        mining::MiningState,
    },
    matchmaking::SeatId,
};

pub(super) struct MatchPlayerComp {
    account_id: Uuid,
    public_player_id: Uuid,
    seat_id: SeatId,
}

impl MatchPlayerComp {
    pub(super) const fn new(account_id: Uuid, public_player_id: Uuid, seat_id: SeatId) -> Self {
        Self {
            account_id,
            public_player_id,
            seat_id,
        }
    }

    pub(super) const fn account_id(&self) -> Uuid {
        self.account_id
    }

    pub(super) const fn public_player_id(&self) -> Uuid {
        self.public_player_id
    }

    pub(super) const fn seat_id(&self) -> SeatId {
        self.seat_id
    }
}

impl Component for MatchPlayerComp {
    type Storage = VecStorage<Self>;
}

pub(super) struct ResourceInventoryComp(MatchInventory);

impl ResourceInventoryComp {
    pub(super) const fn new(inventory: MatchInventory) -> Self {
        Self(inventory)
    }

    pub(super) fn inventory(&self) -> &MatchInventory {
        &self.0
    }

    pub(super) fn inventory_mut(&mut self) -> &mut MatchInventory {
        &mut self.0
    }

    pub(super) fn snapshot(&self) -> InventorySnapshot {
        self.0.snapshot()
    }
}

impl Component for ResourceInventoryComp {
    type Storage = DenseVecStorage<Self>;
}

pub(super) struct FixedEquipmentComp(FixedEquipment);

impl FixedEquipmentComp {
    pub(super) const fn standard() -> Self {
        Self(FixedEquipment::standard())
    }

    pub(super) const fn snapshot(&self) -> FixedEquipmentSnapshot {
        self.0.snapshot()
    }

    pub(super) const fn has_basic_pickaxe(&self) -> bool {
        self.0.has_basic_pickaxe()
    }
}

impl Component for FixedEquipmentComp {
    type Storage = VecStorage<Self>;
}

pub(super) struct MiningComp(MiningState);

impl MiningComp {
    pub(super) fn new() -> Self {
        Self(MiningState::default())
    }

    pub(super) const fn state(&self) -> &MiningState {
        &self.0
    }

    pub(super) fn state_mut(&mut self) -> &mut MiningState {
        &mut self.0
    }
}

impl Component for MiningComp {
    type Storage = DenseVecStorage<Self>;
}

pub(super) struct LootDropComp(LootDrop);

impl LootDropComp {
    pub(super) const fn new(drop: LootDrop) -> Self {
        Self(drop)
    }

    pub(super) fn drop(&self) -> &LootDrop {
        &self.0
    }

    pub(super) fn drop_mut(&mut self) -> &mut LootDrop {
        &mut self.0
    }
}

impl Component for LootDropComp {
    type Storage = DenseVecStorage<Self>;
}
