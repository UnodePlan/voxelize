use std::time::Duration;

pub(crate) const BASIC_MELEE_DAMAGE_HALF_HEARTS: u8 = 2;
pub(crate) const BASIC_MELEE_COOLDOWN: Duration = Duration::from_millis(600);
pub(crate) const BASIC_MELEE_REACH: f32 = 3.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct HealthState {
    half_hearts: u8,
    max_half_hearts: u8,
    revision: u32,
}

impl HealthState {
    pub(crate) fn new(max_half_hearts: u8) -> Result<Self, HealthError> {
        if max_half_hearts == 0 {
            return Err(HealthError::InvalidMaximum);
        }
        Ok(Self {
            half_hearts: max_half_hearts,
            max_half_hearts,
            revision: 0,
        })
    }

    pub(crate) fn half_hearts(&self) -> u8 {
        self.half_hearts
    }

    pub(crate) fn max_half_hearts(&self) -> u8 {
        self.max_half_hearts
    }

    pub(crate) fn revision(&self) -> u32 {
        self.revision
    }

    pub(crate) fn is_alive(&self) -> bool {
        self.half_hearts > 0
    }

    pub(crate) fn apply_damage(
        &mut self,
        damage_half_hearts: u8,
    ) -> Result<DamageOutcome, HealthError> {
        if damage_half_hearts == 0 {
            return Err(HealthError::InvalidDamage);
        }
        if !self.is_alive() {
            return Err(HealthError::AlreadyDead);
        }
        if self.revision == u32::MAX {
            return Err(HealthError::RevisionExhausted);
        }

        self.half_hearts = self.half_hearts.saturating_sub(damage_half_hearts);
        self.revision += 1;
        if self.half_hearts == 0 {
            Ok(DamageOutcome::Killed)
        } else {
            Ok(DamageOutcome::Damaged {
                remaining_half_hearts: self.half_hearts,
            })
        }
    }

    pub(crate) fn eliminate(&mut self) -> Result<DamageOutcome, HealthError> {
        if !self.is_alive() {
            return Err(HealthError::AlreadyDead);
        }
        self.apply_damage(self.half_hearts)
    }

    #[cfg(test)]
    pub(crate) fn set_revision_for_test(&mut self, revision: u32) {
        self.revision = revision;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DamageOutcome {
    Damaged { remaining_half_hearts: u8 },
    Killed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum HealthError {
    InvalidMaximum,
    InvalidDamage,
    AlreadyDead,
    RevisionExhausted,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct CombatState {
    last_sequence: Option<u32>,
    last_attack_at: Option<Duration>,
    revision: u32,
}

impl CombatState {
    pub(crate) fn revision(&self) -> u32 {
        self.revision
    }

    pub(crate) fn last_sequence(&self) -> Option<u32> {
        self.last_sequence
    }

    #[cfg(test)]
    pub(crate) fn last_attack_at(&self) -> Option<Duration> {
        self.last_attack_at
    }

    /// 新鲜序列即使仍在冷却也会被消费，防止同一输入稍后重放命中。
    pub(crate) fn accept_swing(
        &mut self,
        sequence: u32,
        now: Duration,
        cooldown: Duration,
    ) -> Result<SwingOutcome, CombatError> {
        self.validate_sequence(sequence)?;
        if cooldown.is_zero() {
            return Err(CombatError::InvalidCooldown);
        }
        if self.last_attack_at.is_some_and(|last| now < last) {
            return Err(CombatError::TimeRegression);
        }
        if self.revision == u32::MAX {
            return Err(CombatError::RevisionExhausted);
        }

        if let Some(last_attack_at) = self.last_attack_at {
            let ready_at = last_attack_at
                .checked_add(cooldown)
                .ok_or(CombatError::TimeOverflow)?;
            if now < ready_at {
                self.last_sequence = Some(sequence);
                self.revision += 1;
                return Ok(SwingOutcome::Cooldown { ready_at });
            }
        }

        self.last_sequence = Some(sequence);
        self.last_attack_at = Some(now);
        self.revision += 1;
        Ok(SwingOutcome::Accepted)
    }

    #[cfg(test)]
    pub(crate) fn reject_sequence(&mut self, sequence: u32) -> Result<(), CombatError> {
        self.validate_sequence(sequence)?;
        if self.revision == u32::MAX {
            return Err(CombatError::RevisionExhausted);
        }
        self.last_sequence = Some(sequence);
        self.revision += 1;
        Ok(())
    }

    fn validate_sequence(&self, sequence: u32) -> Result<(), CombatError> {
        if self.last_sequence.is_some_and(|last| sequence <= last) {
            Err(CombatError::StaleSequence)
        } else {
            Ok(())
        }
    }

    #[cfg(test)]
    pub(crate) fn set_revision_for_test(&mut self, revision: u32) {
        self.revision = revision;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SwingOutcome {
    Accepted,
    Cooldown { ready_at: Duration },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CombatError {
    StaleSequence,
    InvalidCooldown,
    TimeRegression,
    TimeOverflow,
    RevisionExhausted,
}
