use specs::{Component, DenseVecStorage, VecStorage};
use uuid::Uuid;

use crate::{
    contracts::DeathResultEnvelope,
    gameplay::{
        combat::{CombatState, HealthState},
        equipment::{FixedEquipment, FixedEquipmentSnapshot},
        extraction::ExtractionProgress,
        inventory::{InventorySnapshot, MatchInventory},
        loot::LootDrop,
        mining::MiningState,
        round_stats::RoundStats,
    },
    matchmaking::SeatId,
};

pub(crate) struct MatchPlayerComp {
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

    pub(crate) const fn account_id(&self) -> Uuid {
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

    pub(super) const fn has_basic_melee_weapon(&self) -> bool {
        self.0.has_basic_melee_weapon()
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

pub(super) struct HealthComp(HealthState);

impl HealthComp {
    pub(super) const fn new(health: HealthState) -> Self {
        Self(health)
    }

    pub(super) const fn state(&self) -> &HealthState {
        &self.0
    }

    pub(super) fn state_mut(&mut self) -> &mut HealthState {
        &mut self.0
    }
}

impl Component for HealthComp {
    type Storage = VecStorage<Self>;
}

pub(super) struct CombatComp(CombatState);

impl CombatComp {
    pub(super) const fn new(combat: CombatState) -> Self {
        Self(combat)
    }

    pub(super) const fn state(&self) -> &CombatState {
        &self.0
    }

    pub(super) fn state_mut(&mut self) -> &mut CombatState {
        &mut self.0
    }
}

impl Component for CombatComp {
    type Storage = DenseVecStorage<Self>;
}

pub(super) struct RoundStatsComp(RoundStats);

impl RoundStatsComp {
    pub(super) const fn new(stats: RoundStats) -> Self {
        Self(stats)
    }

    pub(super) const fn stats(&self) -> &RoundStats {
        &self.0
    }

    pub(super) fn stats_mut(&mut self) -> &mut RoundStats {
        &mut self.0
    }
}

impl Component for RoundStatsComp {
    type Storage = DenseVecStorage<Self>;
}

pub(super) struct EliminationRecord {
    pub killer_account_id: Option<Uuid>,
    pub occurred_at: time::OffsetDateTime,
    pub result: DeathResultEnvelope,
    pub notice_sent: bool,
}

pub(crate) struct EliminationComp(Option<EliminationRecord>);

impl EliminationComp {
    pub(super) const fn alive() -> Self {
        Self(None)
    }

    pub(super) fn record(&self) -> Option<&EliminationRecord> {
        self.0.as_ref()
    }

    pub(crate) const fn is_eliminated(&self) -> bool {
        self.0.is_some()
    }

    pub(super) fn record_mut(&mut self) -> Option<&mut EliminationRecord> {
        self.0.as_mut()
    }

    pub(super) fn eliminate(&mut self, record: EliminationRecord) -> bool {
        if self.0.is_some() {
            return false;
        }
        self.0 = Some(record);
        true
    }
}

impl Component for EliminationComp {
    type Storage = VecStorage<Self>;
}

pub(super) struct ExtractionRecord {
    pub qualification: crate::matchmaking::ExtractionQualification,
    pub notice_sent: bool,
}

#[derive(Default)]
pub(crate) struct ExtractionComp {
    progress: ExtractionProgress,
    record: Option<ExtractionRecord>,
    last_published_revision: Option<u32>,
}

impl ExtractionComp {
    pub(super) fn progress(&self) -> &ExtractionProgress {
        &self.progress
    }

    pub(super) fn progress_mut(&mut self) -> &mut ExtractionProgress {
        &mut self.progress
    }

    pub(super) fn record(&self) -> Option<&ExtractionRecord> {
        self.record.as_ref()
    }

    pub(crate) const fn is_settlement_pending(&self) -> bool {
        self.record.is_some()
    }

    pub(super) fn record_mut(&mut self) -> Option<&mut ExtractionRecord> {
        self.record.as_mut()
    }

    pub(super) fn qualify(
        &mut self,
        qualification: crate::matchmaking::ExtractionQualification,
    ) -> bool {
        if self.record.is_some() {
            return false;
        }
        self.record = Some(ExtractionRecord {
            qualification,
            notice_sent: false,
        });
        true
    }

    pub(super) fn should_publish(&mut self, revision: u32) -> bool {
        if self
            .last_published_revision
            .is_some_and(|published| revision <= published)
        {
            return false;
        }
        self.last_published_revision = Some(revision);
        true
    }
}

impl Component for ExtractionComp {
    type Storage = DenseVecStorage<Self>;
}
