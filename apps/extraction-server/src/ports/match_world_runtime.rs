use async_trait::async_trait;
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
        debug_assert_eq!(roster.iter().len(), MATCH_PLAYER_CAPACITY);
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

#[async_trait]
pub trait MatchWorldRuntime: Send + Sync {
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
}
