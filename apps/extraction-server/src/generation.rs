#[cfg(test)]
mod anti_xray_tests;
mod config;
mod deposits;
mod hash;
mod plan;
mod spawn;
mod stage;
#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;

pub(crate) use config::{GenerationConfig, MapPoint};
pub(crate) use plan::{GenerationPlan, MatchMapLayout};
pub(crate) use spawn::install_spawn_assignment;
pub(crate) use stage::install_generation_stage;
