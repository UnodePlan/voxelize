use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use serde::Deserialize;
use voxelize::{DirectionComp, PositionComp, RigidBodyComp, Vec3, World};

use crate::{match_world::PlayableBounds, matchmaking::MatchmakingService};

const MAX_HORIZONTAL_SPEED: f32 = 12.0;
const MOVEMENT_BURST: f32 = 3.0;
const MIN_Y: f32 = -64.0;
const MAX_Y: f32 = 320.0;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MovementUpdate {
    position: Option<Vec3<f32>>,
    direction: Option<Vec3<f32>>,
}

struct MovementBudget {
    available: f32,
    updated_at: Duration,
}

impl MovementBudget {
    fn new(now: Duration) -> Self {
        Self {
            available: MOVEMENT_BURST,
            updated_at: now,
        }
    }

    fn consume(&mut self, distance: f32, now: Duration) -> bool {
        let elapsed = now.saturating_sub(self.updated_at).as_secs_f32();
        self.available = (self.available + elapsed * MAX_HORIZONTAL_SPEED).min(MOVEMENT_BURST);
        self.updated_at = now;
        if distance > self.available {
            return false;
        }
        self.available -= distance;
        true
    }
}

pub(crate) fn install_bounded_movement(
    world: &mut World,
    bounds: PlayableBounds,
    matchmaking: Weak<MatchmakingService>,
    generations: Arc<Mutex<HashMap<String, String>>>,
    world_name: String,
) {
    // 本阶段仅拒绝越界、非有限数、非参赛状态和突发位移。
    // 体素碰撞 sweep、重力与跳跃权威必须在 PVP 公平性验收前补齐。
    let budgets = Arc::new(Mutex::new(HashMap::<u32, MovementBudget>::new()));
    world.set_client_parser(move |world, metadata, entity| {
        let Ok(update) = serde_json::from_str::<MovementUpdate>(metadata) else {
            return;
        };
        let client = world
            .clients()
            .values()
            .find(|client| client.entity == entity)
            .and_then(|client| {
                client
                    .principal
                    .as_ref()
                    .map(|principal| (client.id.clone(), principal.account_id.clone()))
            });
        let Some((client_id, account_id)) = client else {
            return;
        };
        let Some(service) = matchmaking.upgrade() else {
            return;
        };
        let Ok(account_id) = uuid::Uuid::parse_str(&account_id) else {
            service.fail_closed();
            return;
        };
        let Some(world_generation) = generations
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&world_name)
            .cloned()
        else {
            return;
        };
        if !service.allows_gameplay(&world_name, &world_generation, &client_id, account_id) {
            return;
        }
        let now = service.monotonic_now();

        if let Some(position) = update.position {
            let requested = [position.0, position.1, position.2];
            if valid_position(bounds, requested) {
                let current = world
                    .read_component::<PositionComp>()
                    .get(entity)
                    .map(|position| position.0.to_arr());
                if let Some(current) = current {
                    let distance = spatial_distance(current, requested);
                    let allowed = budgets
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .entry(entity.id())
                        .or_insert_with(|| MovementBudget::new(now))
                        .consume(distance, now);
                    if allowed {
                        if let Some(component) =
                            world.write_component::<PositionComp>().get_mut(entity)
                        {
                            component.0.set(requested[0], requested[1], requested[2]);
                        }
                        if let Some(body) = world.write_component::<RigidBodyComp>().get_mut(entity)
                        {
                            body.0
                                .set_position(requested[0], requested[1], requested[2]);
                        }
                    }
                }
            }
        }

        if let Some(direction) = update.direction {
            let direction = [direction.0, direction.1, direction.2];
            if valid_direction(direction) {
                if let Some(component) = world.write_component::<DirectionComp>().get_mut(entity) {
                    component.0.set(direction[0], direction[1], direction[2]);
                }
            }
        }
    });
}

fn valid_position(bounds: PlayableBounds, position: [f32; 3]) -> bool {
    position.iter().all(|value| value.is_finite())
        && bounds.contains_xz(position[0], position[2])
        && (MIN_Y..=MAX_Y).contains(&position[1])
}

fn valid_direction(direction: [f32; 3]) -> bool {
    direction.iter().all(|value| value.is_finite())
        && direction.iter().map(|value| value * value).sum::<f32>() <= 4.0
}

fn spatial_distance(from: [f32; 3], to: [f32; 3]) -> f32 {
    let dx = to[0] - from[0];
    let dy = to[1] - from[1];
    let dz = to[2] - from[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_validation_enforces_finite_exact_bounds() {
        let bounds = PlayableBounds::EXTRACTION;
        assert!(valid_position(bounds, [-150.0, 0.0, 149.999]));
        assert!(!valid_position(bounds, [150.0, 0.0, 0.0]));
        assert!(!valid_position(bounds, [0.0, 0.0, -150.001]));
        assert!(!valid_position(bounds, [f32::NAN, 0.0, 0.0]));
    }

    #[test]
    fn movement_budget_limits_burst_and_refills_with_time() {
        let started_at = Duration::ZERO;
        let mut budget = MovementBudget::new(started_at);
        assert!(budget.consume(3.0, started_at));
        assert!(!budget.consume(0.1, started_at));
        assert!(budget.consume(1.2, started_at + Duration::from_millis(100)));
    }
}
