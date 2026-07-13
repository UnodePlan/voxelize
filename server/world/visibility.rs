use serde::Serialize;

use crate::Vec3;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EntityVisibilityPolicy {
    #[default]
    Legacy,
    Bounded,
}

impl EntityVisibilityPolicy {
    pub const fn legacy() -> Self {
        Self::Legacy
    }

    pub const fn bounded() -> Self {
        Self::Bounded
    }
}

pub(crate) fn bounded_visibility_radius(
    policy: EntityVisibilityPolicy,
    radius: f32,
) -> Option<f32> {
    matches!(policy, EntityVisibilityPolicy::Bounded)
        .then_some(radius)
        .filter(|radius| radius.is_finite() && *radius > 0.0)
}

pub(crate) fn is_position_visible(viewer: &Vec3<f32>, target: &Vec3<f32>, radius: f32) -> bool {
    if !viewer.0.is_finite()
        || !viewer.1.is_finite()
        || !viewer.2.is_finite()
        || !target.0.is_finite()
        || !target.1.is_finite()
        || !target.2.is_finite()
    {
        return false;
    }
    let dx = target.0 - viewer.0;
    let dy = target.1 - viewer.1;
    let dz = target.2 - viewer.2;
    dx * dx + dy * dy + dz * dz <= radius * radius
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visibility_is_inclusive_and_rejects_non_finite_positions() {
        let viewer = Vec3(0.0, 0.0, 0.0);
        assert!(is_position_visible(&viewer, &Vec3(3.0, 4.0, 0.0), 5.0));
        assert!(!is_position_visible(&viewer, &Vec3(3.01, 4.0, 0.0), 5.0));
        assert!(!is_position_visible(
            &viewer,
            &Vec3(f32::NAN, 0.0, 0.0),
            5.0
        ));
    }

    #[test]
    fn bounded_radius_requires_explicit_policy() {
        assert_eq!(
            bounded_visibility_radius(EntityVisibilityPolicy::legacy(), 96.0),
            None
        );
        assert_eq!(
            bounded_visibility_radius(EntityVisibilityPolicy::bounded(), 96.0),
            Some(96.0)
        );
    }
}
