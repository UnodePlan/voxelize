use async_trait::async_trait;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    match_world::{
        FixedMatchLoadout, PlayableBounds, ENGINE_MAX_CHUNK, ENGINE_MIN_CHUNK,
        MATCH_PLAYER_CAPACITY,
    },
    matchmaking::{FrozenRoster, MatchRecord},
};

#[derive(Clone, Debug, PartialEq)]
pub struct MatchWorldSpec {
    pub match_id: Uuid,
    pub world_name: String,
    pub seed: u64,
    pub generation_version: String,
    pub gameplay_version: String,
    pub config_version: String,
    pub roster: FrozenRoster,
    pub engine_min_chunk: [i32; 2],
    pub engine_max_chunk: [i32; 2],
    pub playable_bounds: PlayableBounds,
    pub loadout: FixedMatchLoadout,
    pub saving: bool,
}

impl MatchWorldSpec {
    pub fn from_preparing(record: &MatchRecord, roster: FrozenRoster) -> Self {
        debug_assert!(
            (2..=MATCH_PLAYER_CAPACITY).contains(&roster.len()),
            "roster length must be 2..={MATCH_PLAYER_CAPACITY}, got {}",
            roster.len()
        );
        Self {
            match_id: record.match_id,
            world_name: record.world_name.clone(),
            seed: record.seed,
            generation_version: record.versions.generation.clone(),
            gameplay_version: record.versions.gameplay.clone(),
            config_version: record.versions.config.clone(),
            roster,
            engine_min_chunk: ENGINE_MIN_CHUNK,
            engine_max_chunk: ENGINE_MAX_CHUNK,
            playable_bounds: PlayableBounds::EXTRACTION,
            loadout: FixedMatchLoadout::default(),
            saving: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MatchWorldRuntimeError {
    Conflict,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreparedMatchWorld {
    pub world_generation: String,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MatchWorldRuntimeResourceSnapshot {
    pub generations: usize,
    pub owned_matches: usize,
    pub forced_eliminations: usize,
    pub hard_deadlines: usize,
}

#[async_trait]
pub trait MatchWorldRuntime: Send + Sync {
    fn resource_snapshot(&self) -> MatchWorldRuntimeResourceSnapshot {
        MatchWorldRuntimeResourceSnapshot::default()
    }

    async fn prepare_world(
        &self,
        spec: MatchWorldSpec,
    ) -> Result<PreparedMatchWorld, MatchWorldRuntimeError>;

    async fn stop_world(
        &self,
        match_id: Uuid,
        world_name: &str,
    ) -> Result<bool, MatchWorldRuntimeError>;

    async fn despawn_detached(
        &self,
        world_name: &str,
        account_id: Uuid,
    ) -> Result<bool, MatchWorldRuntimeError>;

    async fn evict_participant(
        &self,
        world_name: &str,
        account_id: Uuid,
    ) -> Result<bool, MatchWorldRuntimeError>;

    async fn request_timeout_elimination(
        &self,
        world_name: &str,
        account_id: Uuid,
    ) -> Result<bool, MatchWorldRuntimeError>;

    async fn seal_hard_deadline(
        &self,
        world_name: &str,
        monotonic_deadline: std::time::Duration,
        utc_deadline: OffsetDateTime,
    ) -> Result<bool, MatchWorldRuntimeError> {
        let _ = (world_name, monotonic_deadline, utc_deadline);
        Err(MatchWorldRuntimeError::Unavailable)
    }
}
