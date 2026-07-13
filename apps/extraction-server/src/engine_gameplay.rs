mod attack_method;
mod authority;
mod auto_pickup;
mod combat_authorization;
mod combat_death;
mod combat_ordering;
mod combat_responses;
mod combat_system;
mod combat_targeting;
mod components;
mod death_outbox;
mod drop_spawn;
mod forced_elimination;
mod intents;
mod manual_drop;
mod messaging;
mod methods;
mod mining_completion;
mod mining_dirty;
mod mining_intent_actions;
mod mining_intents;
mod mining_method;
mod mining_system;
mod mining_validation;
mod runtime;
mod state_method;
mod system;
mod timeout_death;
mod timeout_resolution;

#[cfg(test)]
mod combat_tests;
#[cfg(test)]
mod mining_tests;
#[cfg(test)]
mod tests;

pub(crate) use authority::GameplayAuthority;
pub(crate) use forced_elimination::ForcedEliminationQueue;
pub(crate) use runtime::install_gameplay_runtime;
