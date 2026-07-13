use std::time::Duration;

use serde::Serialize;
use uuid::Uuid;

use super::inventory::{InventoryError, MatchInventory, ResourceStack};
use crate::contracts::ResourceKey;

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub(crate) struct DropId(String);

impl DropId {
    pub(crate) fn manual(match_id: Uuid, seat_id: u8, sequence: u32) -> Self {
        Self(format!(
            "drop:v1:{match_id}:seat:{seat_id}:manual:{sequence}"
        ))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ResourceBundle {
    dirt: u32,
    gold: u32,
    diamond: u32,
}

impl ResourceBundle {
    #[cfg(test)]
    pub(crate) const fn new(dirt: u32, gold: u32, diamond: u32) -> Self {
        Self {
            dirt,
            gold,
            diamond,
        }
    }

    pub(crate) fn from_stack(stack: ResourceStack) -> Self {
        let mut bundle = Self::default();
        bundle.set(stack.resource, stack.quantity);
        bundle
    }

    pub(crate) fn quantity(self, resource: ResourceKey) -> u32 {
        match resource {
            ResourceKey::Dirt => self.dirt,
            ResourceKey::Gold => self.gold,
            ResourceKey::Diamond => self.diamond,
        }
    }

    pub(crate) fn total(self) -> u64 {
        u64::from(self.dirt) + u64::from(self.gold) + u64::from(self.diamond)
    }

    pub(crate) fn is_empty(self) -> bool {
        self.total() == 0
    }

    fn set(&mut self, resource: ResourceKey, quantity: u32) {
        match resource {
            ResourceKey::Dirt => self.dirt = quantity,
            ResourceKey::Gold => self.gold = quantity,
            ResourceKey::Diamond => self.diamond = quantity,
        }
    }

    fn checked_merge(&mut self, other: Self) -> Result<(), LootError> {
        let dirt = self
            .dirt
            .checked_add(other.dirt)
            .ok_or(LootError::QuantityOverflow)?;
        let gold = self
            .gold
            .checked_add(other.gold)
            .ok_or(LootError::QuantityOverflow)?;
        let diamond = self
            .diamond
            .checked_add(other.diamond)
            .ok_or(LootError::QuantityOverflow)?;
        self.dirt = dirt;
        self.gold = gold;
        self.diamond = diamond;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PickupExclusion {
    pub account_id: Uuid,
    pub until: Duration,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LootDrop {
    id: DropId,
    position: [f32; 3],
    contents: ResourceBundle,
    exclusion: Option<PickupExclusion>,
    revision: u32,
}

impl LootDrop {
    pub(crate) fn new(
        id: DropId,
        position: [f32; 3],
        contents: ResourceBundle,
        exclusion: Option<PickupExclusion>,
    ) -> Result<Self, LootError> {
        if contents.is_empty() {
            return Err(LootError::EmptyDrop);
        }
        if !position.into_iter().all(f32::is_finite) {
            return Err(LootError::InvalidPosition);
        }
        Ok(Self {
            id,
            position,
            contents,
            exclusion,
            revision: 0,
        })
    }

    pub(crate) fn id(&self) -> &DropId {
        &self.id
    }

    pub(crate) fn position(&self) -> [f32; 3] {
        self.position
    }

    pub(crate) fn contents(&self) -> ResourceBundle {
        self.contents
    }

    pub(crate) fn revision(&self) -> u32 {
        self.revision
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.contents.is_empty()
    }

    pub(crate) fn excludes(&self, account_id: Uuid, now: Duration) -> bool {
        self.exclusion
            .is_some_and(|exclusion| exclusion.account_id == account_id && now < exclusion.until)
    }

    pub(crate) fn has_active_pickup_exclusion(&self, now: Duration) -> bool {
        self.exclusion
            .is_some_and(|exclusion| now < exclusion.until)
    }

    pub(crate) fn transfer_into(
        &mut self,
        inventory: &mut MatchInventory,
    ) -> Result<u64, LootError> {
        if self.revision == u32::MAX {
            return Err(LootError::RevisionExhausted);
        }
        let entries = ResourceKey::ALL.map(|resource| (resource, self.contents.quantity(resource)));
        let outcomes = inventory
            .insert_batch(&entries)
            .map_err(LootError::Inventory)?;
        let mut accepted_total = 0_u64;
        for ((resource, _), outcome) in entries.into_iter().zip(outcomes) {
            self.contents.set(resource, outcome.remainder);
            accepted_total += u64::from(outcome.accepted);
        }
        if accepted_total > 0 {
            self.revision += 1;
        }
        Ok(accepted_total)
    }

    pub(crate) fn can_merge(
        &self,
        other: &Self,
        bucket_size: f32,
        now: Duration,
    ) -> Result<bool, LootError> {
        Ok(!self.has_active_pickup_exclusion(now)
            && !other.has_active_pickup_exclusion(now)
            && spatial_bucket(self.position, bucket_size)?
                == spatial_bucket(other.position, bucket_size)?)
    }

    pub(crate) fn merge(
        &mut self,
        other: Self,
        bucket_size: f32,
        now: Duration,
    ) -> Result<bool, LootError> {
        if !self.can_merge(&other, bucket_size, now)? {
            return Ok(false);
        }
        if self.revision == u32::MAX {
            return Err(LootError::RevisionExhausted);
        }
        self.contents.checked_merge(other.contents)?;
        self.exclusion = None;
        self.revision += 1;
        Ok(true)
    }
}

fn spatial_bucket(position: [f32; 3], bucket_size: f32) -> Result<[i32; 3], LootError> {
    if !bucket_size.is_finite() || bucket_size <= 0.0 {
        return Err(LootError::InvalidBucketSize);
    }
    Ok([
        (position[0] / bucket_size).floor() as i32,
        (position[1] / bucket_size).floor() as i32,
        (position[2] / bucket_size).floor() as i32,
    ])
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LootError {
    EmptyDrop,
    InvalidPosition,
    InvalidBucketSize,
    QuantityOverflow,
    RevisionExhausted,
    Inventory(InventoryError),
}
