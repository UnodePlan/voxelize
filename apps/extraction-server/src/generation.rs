mod config;
mod deposits;
mod hash;
mod plan;
mod spawn;
mod stage;
#[cfg(test)]
mod tests;

pub(crate) use config::{GenerationConfig, MapPoint};
pub(crate) use plan::{GenerationPlan, MatchMapLayout};
pub(crate) use spawn::install_spawn_assignment;
pub(crate) use stage::install_generation_stage;
