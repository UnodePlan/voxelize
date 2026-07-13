use std::time::Duration;

use super::{
    inventory::ResourceStack,
    loot::{LootError, ResourceBundle},
};
use crate::contracts::ResourceKey;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RoundStats {
    mined: ResourceBundle,
    picked_up: ResourceBundle,
    lost: ResourceBundle,
    kills: u32,
    survival_started_at: Duration,
    survival_ended_at: Option<Duration>,
    revision: u32,
}

impl RoundStats {
    pub(crate) fn new(survival_started_at: Duration) -> Self {
        Self {
            mined: ResourceBundle::default(),
            picked_up: ResourceBundle::default(),
            lost: ResourceBundle::default(),
            kills: 0,
            survival_started_at,
            survival_ended_at: None,
            revision: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn revision(&self) -> u32 {
        self.revision
    }

    pub(crate) fn mined(&self) -> ResourceBundle {
        self.mined
    }

    pub(crate) fn picked_up(&self) -> ResourceBundle {
        self.picked_up
    }

    pub(crate) fn lost(&self) -> ResourceBundle {
        self.lost
    }

    #[cfg(test)]
    pub(crate) fn kills(&self) -> u32 {
        self.kills
    }

    pub(crate) fn survival_started_at(&self) -> Duration {
        self.survival_started_at
    }

    pub(crate) fn survival_ended_at(&self) -> Option<Duration> {
        self.survival_ended_at
    }

    pub(crate) fn record_mined(
        &mut self,
        resource: ResourceKey,
        quantity: u32,
    ) -> Result<bool, RoundStatsError> {
        if quantity == 0 {
            return Ok(false);
        }
        self.record_bundle(
            StatBucket::Mined,
            ResourceBundle::from_stack(ResourceStack { resource, quantity }),
        )
    }

    pub(crate) fn record_picked_up(
        &mut self,
        bundle: ResourceBundle,
    ) -> Result<bool, RoundStatsError> {
        self.record_bundle(StatBucket::PickedUp, bundle)
    }

    pub(crate) fn record_lost(&mut self, bundle: ResourceBundle) -> Result<bool, RoundStatsError> {
        self.record_bundle(StatBucket::Lost, bundle)
    }

    pub(crate) fn record_kill(&mut self) -> Result<(), RoundStatsError> {
        let kills = self
            .kills
            .checked_add(1)
            .ok_or(RoundStatsError::QuantityOverflow)?;
        self.ensure_revision_available()?;
        self.kills = kills;
        self.revision += 1;
        Ok(())
    }

    pub(crate) fn finish_survival(&mut self, ended_at: Duration) -> Result<bool, RoundStatsError> {
        if self.survival_ended_at.is_some() {
            return Ok(false);
        }
        if ended_at < self.survival_started_at {
            return Err(RoundStatsError::TimeRegression);
        }
        self.ensure_revision_available()?;
        self.survival_ended_at = Some(ended_at);
        self.revision += 1;
        Ok(true)
    }

    fn record_bundle(
        &mut self,
        bucket: StatBucket,
        bundle: ResourceBundle,
    ) -> Result<bool, RoundStatsError> {
        if bundle.is_empty() {
            return Ok(false);
        }
        self.ensure_revision_available()?;
        let destination = match bucket {
            StatBucket::Mined => &mut self.mined,
            StatBucket::PickedUp => &mut self.picked_up,
            StatBucket::Lost => &mut self.lost,
        };
        let mut candidate = *destination;
        candidate.checked_merge(bundle).map_err(map_loot_error)?;
        *destination = candidate;
        self.revision += 1;
        Ok(true)
    }

    fn ensure_revision_available(&self) -> Result<(), RoundStatsError> {
        if self.revision == u32::MAX {
            Err(RoundStatsError::RevisionExhausted)
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    pub(crate) fn set_revision_for_test(&mut self, revision: u32) {
        self.revision = revision;
    }
}

fn map_loot_error(error: LootError) -> RoundStatsError {
    match error {
        LootError::QuantityOverflow => RoundStatsError::QuantityOverflow,
        _ => RoundStatsError::InvariantViolation,
    }
}

#[derive(Clone, Copy)]
enum StatBucket {
    Mined,
    PickedUp,
    Lost,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RoundStatsError {
    QuantityOverflow,
    RevisionExhausted,
    TimeRegression,
    InvariantViolation,
}
