#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RayBounds {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

pub(crate) fn ray_aabb_distance(
    origin: [f32; 3],
    direction: [f32; 3],
    bounds: RayBounds,
    max_distance: f32,
) -> Result<Option<f32>, RayAabbError> {
    validate_inputs(origin, direction, bounds, max_distance)?;

    let length_squared = direction
        .into_iter()
        .map(|value| value * value)
        .sum::<f32>();
    let length = length_squared.sqrt();
    let direction = direction.map(|value| value / length);
    let mut enter = 0.0_f32;
    let mut exit = max_distance;

    for axis in 0..3 {
        if direction[axis].abs() <= f32::EPSILON {
            if origin[axis] < bounds.min[axis] || origin[axis] > bounds.max[axis] {
                return Ok(None);
            }
            continue;
        }

        let inverse = 1.0 / direction[axis];
        let first = (bounds.min[axis] - origin[axis]) * inverse;
        let second = (bounds.max[axis] - origin[axis]) * inverse;
        enter = enter.max(first.min(second));
        exit = exit.min(first.max(second));
        if enter > exit {
            return Ok(None);
        }
    }

    Ok(Some(enter))
}

fn validate_inputs(
    origin: [f32; 3],
    direction: [f32; 3],
    bounds: RayBounds,
    max_distance: f32,
) -> Result<(), RayAabbError> {
    if !origin.into_iter().all(f32::is_finite) {
        return Err(RayAabbError::Origin);
    }
    if !direction.into_iter().all(f32::is_finite) {
        return Err(RayAabbError::Direction);
    }
    let length_squared = direction
        .into_iter()
        .map(|value| value * value)
        .sum::<f32>();
    if !length_squared.is_finite() || length_squared <= f32::EPSILON {
        return Err(RayAabbError::Direction);
    }
    if !bounds.min.into_iter().all(f32::is_finite)
        || !bounds.max.into_iter().all(f32::is_finite)
        || (0..3).any(|axis| bounds.min[axis] > bounds.max[axis])
    {
        return Err(RayAabbError::Bounds);
    }
    if !max_distance.is_finite() || max_distance < 0.0 {
        return Err(RayAabbError::MaxDistance);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum RayAabbError {
    Origin,
    Direction,
    Bounds,
    MaxDistance,
}
