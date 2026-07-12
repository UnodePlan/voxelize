use serde::Serialize;

/// Client input policy applied before custom Method/Event/parser callbacks.
///
/// Registering a handler does not authorize it in strict mode. Public games
/// must separately allow each client-originated Method and Event name.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum WorldRequestPolicy {
    Legacy,
    Strict {
        allowed_methods: Vec<String>,
        allowed_events: Vec<String>,
    },
}

impl Default for WorldRequestPolicy {
    fn default() -> Self {
        Self::legacy()
    }
}

impl WorldRequestPolicy {
    pub const fn legacy() -> Self {
        Self::Legacy
    }

    pub const fn strict() -> Self {
        Self::Strict {
            allowed_methods: Vec::new(),
            allowed_events: Vec::new(),
        }
    }

    pub fn allow_method(mut self, method: impl Into<String>) -> Self {
        if let Self::Strict {
            allowed_methods, ..
        } = &mut self
        {
            insert_normalized(allowed_methods, method.into());
        }
        self
    }

    pub fn allow_event(mut self, event: impl Into<String>) -> Self {
        if let Self::Strict { allowed_events, .. } = &mut self {
            insert_normalized(allowed_events, event.into());
        }
        self
    }

    pub fn is_strict(&self) -> bool {
        matches!(self, Self::Strict { .. })
    }

    pub(crate) fn allows_raw_voxel_updates(&self) -> bool {
        matches!(self, Self::Legacy)
    }

    pub(crate) fn allows_client_movement_flags(&self) -> bool {
        matches!(self, Self::Legacy)
    }

    pub(crate) fn allows_commands(&self) -> bool {
        matches!(self, Self::Legacy)
    }

    pub(crate) fn allows_method(&self, method: &str) -> bool {
        match self {
            Self::Legacy => true,
            Self::Strict {
                allowed_methods, ..
            } => contains_normalized(allowed_methods, method),
        }
    }

    pub(crate) fn allows_event(&self, event: &str) -> bool {
        match self {
            Self::Legacy => true,
            Self::Strict { allowed_events, .. } => contains_normalized(allowed_events, event),
        }
    }
}

fn insert_normalized(values: &mut Vec<String>, value: String) {
    let value = value.to_lowercase();
    if !values.contains(&value) {
        values.push(value);
        values.sort();
    }
}

fn contains_normalized(values: &[String], value: &str) -> bool {
    let normalized = value.to_lowercase();
    values.iter().any(|allowed| allowed == &normalized)
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClientDisconnectPolicy {
    #[default]
    Despawn,
    Detach,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_policy_requires_explicit_normalized_names() {
        let policy = WorldRequestPolicy::strict()
            .allow_method("PVP:V1:Attack")
            .allow_event("pvp:v1:mining");

        assert!(!policy.allows_raw_voxel_updates());
        assert!(!policy.allows_client_movement_flags());
        assert!(!policy.allows_commands());
        assert!(policy.allows_method("pvp:v1:attack"));
        assert!(policy.allows_event("PVP:V1:MINING"));
        assert!(!policy.allows_method("vox-builtin:set-time"));
    }
}
