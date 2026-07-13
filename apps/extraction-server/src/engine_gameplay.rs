mod authority;
mod auto_pickup;
mod components;
mod drop_spawn;
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
mod system;

#[cfg(test)]
mod mining_tests;
#[cfg(test)]
mod tests;

pub(crate) use authority::GameplayAuthority;
pub(crate) use runtime::install_gameplay_runtime;
