use std::time::{Duration, Instant, SystemTime};

use uuid::Uuid;

pub trait Clock: Send + Sync {
    fn monotonic_now(&self) -> Duration;
    fn utc_now(&self) -> SystemTime;
}

#[derive(Clone, Debug)]
pub struct SystemClock {
    started_at: Instant,
    utc_started_at: SystemTime,
}

impl Default for SystemClock {
    fn default() -> Self {
        Self {
            started_at: Instant::now(),
            utc_started_at: SystemTime::now(),
        }
    }
}

impl Clock for SystemClock {
    fn monotonic_now(&self) -> Duration {
        self.started_at.elapsed()
    }

    fn utc_now(&self) -> SystemTime {
        self.utc_started_at
            .checked_add(self.started_at.elapsed())
            .unwrap_or(self.utc_started_at)
    }
}

pub trait IdGenerator: Send + Sync {
    fn next_uuid(&self) -> Uuid;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RandomIdGenerator;

impl IdGenerator for RandomIdGenerator {
    fn next_uuid(&self) -> Uuid {
        Uuid::new_v4()
    }
}

pub trait SeedGenerator: Send + Sync {
    fn next_seed(&self) -> u64;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RandomSeedGenerator;

impl SeedGenerator for RandomSeedGenerator {
    fn next_seed(&self) -> u64 {
        // PostgreSQL 使用有符号 BIGINT；保留 63 位随机性并保证可无损持久化。
        (Uuid::new_v4().as_u128() as u64) & i64::MAX as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_utc_and_monotonic_time_share_one_elapsed_source() {
        let clock = SystemClock::default();
        std::thread::sleep(Duration::from_millis(2));

        let monotonic_before = clock.monotonic_now();
        let utc_elapsed = clock
            .utc_now()
            .duration_since(clock.utc_started_at)
            .unwrap();
        let monotonic_after = clock.monotonic_now();

        assert!(utc_elapsed >= monotonic_before);
        assert!(utc_elapsed <= monotonic_after);
    }
}
