mod system;

#[cfg(test)]
mod tests;

use std::time::Duration;

use serde::{de::IgnoredAny, Deserialize};
use specs::{Component, VecStorage, WorldExt};
use voxelize::{DirectionComp, DispatcherHookError, PositionComp, RigidBodyComp, World};

use crate::{
    engine_gameplay::GameplayAuthority,
    match_world::{PlayableBounds, PLAYER_EYE_OFFSET_FROM_CENTER},
};

use self::system::AuthoritativeMovementSystem;

pub(crate) const MOVEMENT_SYSTEM_NAME: &str = "extraction-authoritative-movement";

const INPUTS_PER_SECOND: f32 = 30.0;
const INPUT_BURST: f32 = 6.0;
const INPUT_STALE_AFTER: Duration = Duration::from_millis(250);

struct MovementInstallMarker;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MovementUpdate {
    movement: MovementAxes,
    direction: [f32; 3],
    #[serde(default, rename = "position")]
    _ignored_position: Option<IgnoredAny>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct MovementAxes {
    forward: f32,
    right: f32,
    jump: bool,
}

#[derive(Clone, Copy)]
struct AcceptedMovement {
    forward: f32,
    right: f32,
    jump: bool,
    direction: [f32; 3],
    horizontal_forward: [f32; 2],
}

pub(super) struct MovementIntentComp {
    forward: f32,
    right: f32,
    horizontal_forward: [f32; 2],
    jump_held: bool,
    jump_pending: bool,
    last_input_at: Option<Duration>,
    budget_updated_at: Option<Duration>,
    available_inputs: f32,
}

impl Default for MovementIntentComp {
    fn default() -> Self {
        Self {
            forward: 0.0,
            right: 0.0,
            horizontal_forward: [0.0, -1.0],
            jump_held: false,
            jump_pending: false,
            last_input_at: None,
            budget_updated_at: None,
            available_inputs: INPUT_BURST,
        }
    }
}

impl Component for MovementIntentComp {
    type Storage = VecStorage<Self>;
}

impl MovementIntentComp {
    fn accept(&mut self, update: MovementUpdate, now: Duration) -> Option<[f32; 3]> {
        let Some(accepted) = validate_update(update) else {
            self.stop();
            return None;
        };
        if !self.consume_budget(now) {
            self.stop();
            return None;
        }

        self.forward = accepted.forward;
        self.right = accepted.right;
        self.horizontal_forward = accepted.horizontal_forward;
        // 只在 false -> true 时排队一次；保持按键不会在落地后自动再次起跳。
        self.jump_pending |= accepted.jump && !self.jump_held;
        self.jump_held = accepted.jump;
        self.last_input_at = Some(now);
        Some(accepted.direction)
    }

    fn take_control(&mut self, now: Duration) -> Option<([f32; 2], bool)> {
        let fresh = self.last_input_at.is_some_and(|accepted_at| {
            now >= accepted_at && now - accepted_at <= INPUT_STALE_AFTER
        });
        if !fresh {
            self.stop();
            return None;
        }
        let right = [-self.horizontal_forward[1], self.horizontal_forward[0]];
        let horizontal = [
            self.horizontal_forward[0] * self.forward + right[0] * self.right,
            self.horizontal_forward[1] * self.forward + right[1] * self.right,
        ];
        let jump = std::mem::take(&mut self.jump_pending);
        Some((horizontal, jump))
    }

    fn stop(&mut self) {
        self.forward = 0.0;
        self.right = 0.0;
        self.jump_pending = false;
    }

    fn consume_budget(&mut self, now: Duration) -> bool {
        if self
            .budget_updated_at
            .is_some_and(|updated_at| now < updated_at)
        {
            return false;
        }
        if let Some(updated_at) = self.budget_updated_at {
            let refill = (now - updated_at).as_secs_f32() * INPUTS_PER_SECOND;
            self.available_inputs = (self.available_inputs + refill).min(INPUT_BURST);
        }
        self.budget_updated_at = Some(now);
        if self.available_inputs < 1.0 {
            return false;
        }
        self.available_inputs -= 1.0;
        true
    }
}

pub(crate) fn install_bounded_movement(
    world: &mut World,
    bounds: PlayableBounds,
    authority: GameplayAuthority,
) -> Result<(), DispatcherHookError> {
    world.ecs_mut().register::<MovementIntentComp>();
    world.ecs_mut().insert(bounds);
    world.ecs_mut().insert(MovementInstallMarker);
    world.add_client_modifier(|world, entity| {
        // PositionComp 对外仍表示眼睛位置；Voxelize 刚体内部使用碰撞盒中心。
        let eye_position = world
            .write_component::<RigidBodyComp>()
            .get_mut(entity)
            .map(|body| {
                body.0.gravity_multiplier = 1.0;
                body.0.auto_step = false;
                let center = body.0.get_position();
                [center.0, center.1 + PLAYER_EYE_OFFSET_FROM_CENTER, center.2]
            });
        if let Some(eye_position) = eye_position {
            if let Some(position) = world.write_component::<PositionComp>().get_mut(entity) {
                position
                    .0
                    .set(eye_position[0], eye_position[1], eye_position[2]);
            }
        }
        world.add(entity, MovementIntentComp::default());
    });

    let parser_authority = authority.clone();
    world.set_client_parser(move |world, metadata, entity| {
        let Some((_account_id, now)) = parser_authority.authorize_entity(world, entity) else {
            stop_entity(world, entity);
            return;
        };
        let Ok(update) = serde_json::from_str::<MovementUpdate>(metadata) else {
            stop_entity(world, entity);
            return;
        };
        if !world.read_component::<DirectionComp>().contains(entity) {
            stop_entity(world, entity);
            return;
        }
        let direction = world
            .write_component::<MovementIntentComp>()
            .get_mut(entity)
            .and_then(|intent| intent.accept(update, now));
        if let Some(direction) = direction {
            if let Some(component) = world.write_component::<DirectionComp>().get_mut(entity) {
                component.0.set(direction[0], direction[1], direction[2]);
            }
        }
    });

    world.install_before_spatial_update_system(MOVEMENT_SYSTEM_NAME, || {
        AuthoritativeMovementSystem
    })?;
    Ok(())
}

pub(crate) fn is_authoritative_movement_installed(world: &World) -> bool {
    world.ecs().try_fetch::<MovementInstallMarker>().is_some()
}

fn stop_entity(world: &mut World, entity: specs::Entity) {
    if let Some(intent) = world
        .write_component::<MovementIntentComp>()
        .get_mut(entity)
    {
        intent.stop();
    }
}

fn validate_update(update: MovementUpdate) -> Option<AcceptedMovement> {
    let MovementAxes {
        forward,
        right,
        jump,
    } = update.movement;
    let axes_length_squared = forward * forward + right * right;
    if !forward.is_finite()
        || !right.is_finite()
        || !(-1.0..=1.0).contains(&forward)
        || !(-1.0..=1.0).contains(&right)
        || !axes_length_squared.is_finite()
        || axes_length_squared > 1.0
    {
        return None;
    }
    if !update.direction.into_iter().all(f32::is_finite) {
        return None;
    }
    let length_squared = update
        .direction
        .into_iter()
        .map(|value| value * value)
        .sum::<f32>();
    let horizontal_squared =
        update.direction[0] * update.direction[0] + update.direction[2] * update.direction[2];
    if !length_squared.is_finite()
        || length_squared <= f32::EPSILON
        || !horizontal_squared.is_finite()
        || horizontal_squared <= f32::EPSILON
    {
        return None;
    }
    let length = length_squared.sqrt();
    let horizontal_length = horizontal_squared.sqrt();
    Some(AcceptedMovement {
        forward,
        right,
        jump,
        direction: update.direction.map(|value| value / length),
        horizontal_forward: [
            update.direction[0] / horizontal_length,
            update.direction[2] / horizontal_length,
        ],
    })
}
