use std::time::{Duration, SystemTime};

use extraction_server::ports::{Clock, IdGenerator, SeedGenerator};
use uuid::Uuid;

#[test]
fn domain_sources_are_replaceable_with_deterministic_fakes() {
    let clock: &dyn Clock = &FixedClock;
    let ids: &dyn IdGenerator = &FixedIdGenerator;
    let seeds: &dyn SeedGenerator = &FixedSeedGenerator;

    assert_eq!(clock.monotonic_now(), Duration::from_secs(12));
    assert_eq!(clock.utc_now(), SystemTime::UNIX_EPOCH);
    assert_eq!(ids.next_uuid(), Uuid::nil());
    assert_eq!(seeds.next_seed(), 42);
}

struct FixedClock;

impl Clock for FixedClock {
    fn monotonic_now(&self) -> Duration {
        Duration::from_secs(12)
    }

    fn utc_now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH
    }
}

struct FixedIdGenerator;

impl IdGenerator for FixedIdGenerator {
    fn next_uuid(&self) -> Uuid {
        Uuid::nil()
    }
}

struct FixedSeedGenerator;

impl SeedGenerator for FixedSeedGenerator {
    fn next_seed(&self) -> u64 {
        42
    }
}
