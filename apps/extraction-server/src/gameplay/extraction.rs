use std::time::Duration;

use time::OffsetDateTime;
use uuid::Uuid;

use super::inventory::{InventoryError, MatchInventory};
use crate::matchmaking::{ExtractionQualification, ParticipantMatchStats, SettlementResources};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct ExtractionZone {
    center: [f32; 3],
    radius: f32,
    half_height: f32,
}

impl ExtractionZone {
    pub(crate) fn new(
        center: [f32; 3],
        radius: f32,
        half_height: f32,
    ) -> Result<Self, ExtractionProgressError> {
        if !center.into_iter().all(f32::is_finite)
            || !radius.is_finite()
            || radius <= 0.0
            || !half_height.is_finite()
            || half_height <= 0.0
        {
            return Err(ExtractionProgressError::InvalidConfig);
        }
        Ok(Self {
            center,
            radius,
            half_height,
        })
    }

    pub(crate) fn contains(self, position: [f32; 3]) -> bool {
        if !position.into_iter().all(f32::is_finite) {
            return false;
        }
        let dx = position[0] - self.center[0];
        let dz = position[2] - self.center[2];
        dx.mul_add(dx, dz * dz) <= self.radius * self.radius
            && (position[1] - self.center[1]).abs() <= self.half_height
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct ExtractionProgress {
    entered_at: Option<Duration>,
    qualified_at: Option<Duration>,
    revision: u32,
}

impl ExtractionProgress {
    #[cfg(feature = "engine")]
    pub(crate) const fn revision(&self) -> u32 {
        self.revision
    }

    #[cfg(feature = "engine")]
    pub(crate) fn elapsed(&self, now: Duration, hard_deadline: Duration) -> Duration {
        self.entered_at
            .map(|entered_at| now.min(hard_deadline).saturating_sub(entered_at))
            .unwrap_or_default()
    }

    pub(crate) fn observe(
        &mut self,
        now: Duration,
        hard_deadline: Duration,
        eligible: bool,
        inside: bool,
        required: Duration,
    ) -> Result<ExtractionProgressOutcome, ExtractionProgressError> {
        if required.is_zero() {
            return Err(ExtractionProgressError::InvalidConfig);
        }
        if let Some(qualified_at) = self.qualified_at {
            return Ok(ExtractionProgressOutcome::AlreadyQualified { qualified_at });
        }
        if !eligible || !inside {
            return self.reset();
        }

        let evaluated_at = now.min(hard_deadline);
        let Some(entered_at) = self.entered_at else {
            if now > hard_deadline {
                return Ok(ExtractionProgressOutcome::Expired);
            }
            self.bump_revision()?;
            self.entered_at = Some(now);
            return Ok(ExtractionProgressOutcome::Started { entered_at: now });
        };
        if evaluated_at < entered_at {
            return Err(ExtractionProgressError::ClockMovedBackwards);
        }
        if evaluated_at - entered_at < required {
            self.bump_revision()?;
            return Ok(if now > hard_deadline {
                ExtractionProgressOutcome::Expired
            } else {
                ExtractionProgressOutcome::Accumulating {
                    entered_at,
                    elapsed: evaluated_at - entered_at,
                }
            });
        }

        self.bump_revision()?;
        self.qualified_at = Some(evaluated_at);
        Ok(ExtractionProgressOutcome::Qualified {
            qualified_at: evaluated_at,
        })
    }

    fn reset(&mut self) -> Result<ExtractionProgressOutcome, ExtractionProgressError> {
        if self.entered_at.take().is_some() {
            self.bump_revision()?;
            Ok(ExtractionProgressOutcome::Reset)
        } else {
            Ok(ExtractionProgressOutcome::Idle)
        }
    }

    fn bump_revision(&mut self) -> Result<(), ExtractionProgressError> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(ExtractionProgressError::RevisionExhausted)?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExtractionProgressOutcome {
    Idle,
    Started {
        entered_at: Duration,
    },
    Accumulating {
        entered_at: Duration,
        elapsed: Duration,
    },
    Reset,
    Qualified {
        qualified_at: Duration,
    },
    AlreadyQualified {
        qualified_at: Duration,
    },
    Expired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExtractionProgressError {
    InvalidConfig,
    ClockMovedBackwards,
    RevisionExhausted,
}

pub(crate) fn freeze_inventory_for_extraction(
    inventory: &mut MatchInventory,
    match_id: Uuid,
    account_id: Uuid,
    qualified_at: OffsetDateTime,
    stats: ParticipantMatchStats,
    config_version: &str,
) -> Result<ExtractionQualification, ExtractionFreezeError> {
    let snapshot = inventory.snapshot();
    let mut dirt = 0_u64;
    let mut gold = 0_u64;
    let mut diamond = 0_u64;
    for stack in snapshot.slots.into_iter().flatten() {
        let target = match stack.resource {
            crate::contracts::ResourceKey::Dirt => &mut dirt,
            crate::contracts::ResourceKey::Gold => &mut gold,
            crate::contracts::ResourceKey::Diamond => &mut diamond,
        };
        *target = target
            .checked_add(u64::from(stack.quantity))
            .ok_or(ExtractionFreezeError::QuantityOverflow)?;
    }
    let qualification = ExtractionQualification::new(
        match_id,
        account_id,
        qualified_at,
        SettlementResources::new(dirt, gold, diamond),
        stats,
        config_version.to_owned(),
    )
    .ok_or(ExtractionFreezeError::InvalidQualification)?;
    let mut candidate = inventory.clone();
    candidate
        .freeze()
        .map_err(ExtractionFreezeError::Inventory)?;
    *inventory = candidate;
    Ok(qualification)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ExtractionFreezeError {
    QuantityOverflow,
    InvalidQualification,
    Inventory(InventoryError),
}
