use std::time::Duration;

use crate::contracts::{MiningIdleReason, MiningStateData, ResourceKey};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct VoxelCoordinate {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl VoxelCoordinate {
    pub(crate) const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub(crate) const fn as_array(self) -> [i32; 3] {
        [self.x, self.y, self.z]
    }
}

impl From<[i32; 3]> for VoxelCoordinate {
    fn from([x, y, z]: [i32; 3]) -> Self {
        Self::new(x, y, z)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MiningTarget {
    pub voxel: VoxelCoordinate,
    pub voxel_id: u32,
    pub resource: ResourceKey,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct MiningAttempt {
    target: MiningTarget,
    started_at: Duration,
    last_maintained_at: Duration,
    reported_elapsed: Duration,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MiningState {
    last_sequence: Option<u32>,
    revision: u32,
    active: Option<MiningAttempt>,
    idle_reason: MiningIdleReason,
}

impl Default for MiningState {
    fn default() -> Self {
        Self {
            last_sequence: None,
            revision: 0,
            active: None,
            idle_reason: MiningIdleReason::Initial,
        }
    }
}

impl MiningState {
    pub(crate) const fn revision(&self) -> u32 {
        self.revision
    }

    pub(crate) const fn active_target(&self) -> Option<MiningTarget> {
        match self.active {
            Some(attempt) => Some(attempt.target),
            None => None,
        }
    }

    pub(crate) fn ready_at(&self, required: Duration) -> Option<Duration> {
        self.active
            .and_then(|attempt| attempt.started_at.checked_add(required))
    }

    pub(crate) fn start(
        &mut self,
        sequence: u32,
        target: MiningTarget,
        now: Duration,
    ) -> Result<(), MiningStateError> {
        self.ensure_new_sequence(sequence)?;
        self.ensure_revision_available()?;
        self.last_sequence = Some(sequence);
        self.active = Some(MiningAttempt {
            target,
            started_at: now,
            last_maintained_at: now,
            reported_elapsed: Duration::ZERO,
        });
        self.revision += 1;
        Ok(())
    }

    pub(crate) fn maintain(
        &mut self,
        sequence: u32,
        now: Duration,
    ) -> Result<(), MiningStateError> {
        self.ensure_new_sequence(sequence)?;
        self.ensure_revision_available()?;
        let Some(attempt) = self.active.as_mut() else {
            self.last_sequence = Some(sequence);
            self.idle_reason = MiningIdleReason::InvalidBlock;
            self.revision += 1;
            return Err(MiningStateError::NoActiveAttempt);
        };
        if now < attempt.started_at || now < attempt.last_maintained_at {
            return Err(MiningStateError::TimeRegression);
        }
        attempt.last_maintained_at = now;
        self.last_sequence = Some(sequence);
        self.revision += 1;
        Ok(())
    }

    pub(crate) fn cancel(&mut self, sequence: u32) -> Result<(), MiningStateError> {
        self.accept_idle(sequence, MiningIdleReason::Cancelled)
    }

    #[cfg(feature = "engine")]
    pub(crate) fn reject(
        &mut self,
        sequence: u32,
        reason: MiningIdleReason,
    ) -> Result<(), MiningStateError> {
        self.accept_idle(sequence, reason)
    }

    pub(crate) fn reset(&mut self, reason: MiningIdleReason) -> Result<bool, MiningStateError> {
        if self.active.is_none() {
            return Ok(false);
        }
        self.ensure_revision_available()?;
        self.active = None;
        self.idle_reason = reason;
        self.revision += 1;
        Ok(true)
    }

    pub(crate) fn sample(
        &mut self,
        now: Duration,
        required: Duration,
        maintain_grace: Duration,
        sync_interval: Duration,
    ) -> Result<MiningTick, MiningStateError> {
        let Some(attempt) = self.active else {
            return Ok(MiningTick::Idle);
        };
        if required.is_zero() || sync_interval.is_zero() {
            return Err(MiningStateError::InvalidDuration);
        }
        let required_ms = duration_millis(required)?;
        let interval_ms = duration_millis(sync_interval)?;
        if required_ms == 0 || interval_ms == 0 {
            return Err(MiningStateError::InvalidDuration);
        }
        if now < attempt.started_at || now < attempt.last_maintained_at {
            self.reset(MiningIdleReason::TimedOut)?;
            return Ok(MiningTick::Reset);
        }
        if now - attempt.last_maintained_at > maintain_grace {
            self.reset(MiningIdleReason::TimedOut)?;
            return Ok(MiningTick::Reset);
        }
        let elapsed = now - attempt.started_at;
        if elapsed >= required {
            return Ok(MiningTick::Ready(attempt.target));
        }

        let elapsed_ms = duration_millis(elapsed)?;
        let reported = Duration::from_millis(u64::from(elapsed_ms / interval_ms * interval_ms));
        if reported > attempt.reported_elapsed {
            self.ensure_revision_available()?;
            self.active.as_mut().unwrap().reported_elapsed = reported;
            self.revision += 1;
            return Ok(MiningTick::Progressed);
        }
        Ok(MiningTick::Unchanged)
    }

    pub(crate) fn completed(&self) -> Result<Self, MiningStateError> {
        let mut completed = self.clone();
        completed.complete()?;
        Ok(completed)
    }

    fn complete(&mut self) -> Result<(), MiningStateError> {
        if self.active.is_none() {
            return Err(MiningStateError::NoActiveAttempt);
        }
        self.ensure_revision_available()?;
        self.active = None;
        self.idle_reason = MiningIdleReason::Completed;
        self.revision += 1;
        Ok(())
    }

    pub(crate) fn snapshot(
        &self,
        required: Option<Duration>,
    ) -> Result<MiningStateData, MiningStateError> {
        match self.active {
            Some(attempt) => Ok(MiningStateData::Mining {
                accepted_sequence: self
                    .last_sequence
                    .ok_or(MiningStateError::InvariantViolation)?,
                target: attempt.target.voxel.as_array(),
                resource: attempt.target.resource,
                elapsed_ms: duration_millis(attempt.reported_elapsed)?,
                required_ms: required
                    .ok_or(MiningStateError::InvariantViolation)
                    .and_then(duration_millis)?,
            }),
            None => Ok(MiningStateData::Idle {
                accepted_sequence: self.last_sequence,
                reason: self.idle_reason,
            }),
        }
    }

    fn accept_idle(
        &mut self,
        sequence: u32,
        reason: MiningIdleReason,
    ) -> Result<(), MiningStateError> {
        self.ensure_new_sequence(sequence)?;
        self.ensure_revision_available()?;
        self.last_sequence = Some(sequence);
        self.active = None;
        self.idle_reason = reason;
        self.revision += 1;
        Ok(())
    }

    fn ensure_new_sequence(&self, sequence: u32) -> Result<(), MiningStateError> {
        if self.last_sequence.is_some_and(|last| sequence <= last) {
            Err(MiningStateError::StaleSequence)
        } else {
            Ok(())
        }
    }

    fn ensure_revision_available(&self) -> Result<(), MiningStateError> {
        (self.revision < u32::MAX)
            .then_some(())
            .ok_or(MiningStateError::RevisionExhausted)
    }
}

fn duration_millis(duration: Duration) -> Result<u32, MiningStateError> {
    u32::try_from(duration.as_millis()).map_err(|_| MiningStateError::DurationOverflow)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MiningTick {
    Idle,
    Unchanged,
    Progressed,
    Reset,
    Ready(MiningTarget),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MiningStateError {
    StaleSequence,
    NoActiveAttempt,
    TimeRegression,
    RevisionExhausted,
    InvalidDuration,
    DurationOverflow,
    InvariantViolation,
}
