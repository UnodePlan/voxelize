use std::{collections::HashMap, sync::Arc};

use voxelize::{DirectionComp, PositionComp, RigidBodyComp, World};

use super::{MapPoint, MatchMapLayout};
use crate::matchmaking::FrozenRoster;

pub(crate) fn install_spawn_assignment(
    world: &mut World,
    roster: &FrozenRoster,
    layout: &MatchMapLayout,
) {
    let assignments = roster
        .iter()
        .filter_map(|participant| {
            layout
                .spawn_for_seat(participant.seat_id.get() as usize)
                .map(|point| (participant.public_player_id.to_string(), point))
        })
        .collect::<HashMap<_, _>>();
    let assignments = Arc::new(assignments);

    world.set_client_modifier(move |world, entity| {
        let client_id = world.get_id(entity);
        let Some(point) = assignments.get(&client_id).copied() else {
            return;
        };
        if let Some(position) = world.write_component::<PositionComp>().get_mut(entity) {
            position
                .0
                .set(point.x as f32, point.y as f32, point.z as f32);
        }
        if let Some(body) = world.write_component::<RigidBodyComp>().get_mut(entity) {
            body.0
                .set_position(point.x as f32, point.y as f32, point.z as f32);
        }
        if let Some(direction) = world.write_component::<DirectionComp>().get_mut(entity) {
            let facing = direction_toward_center(point);
            direction.0.set(facing[0], 0.0, facing[1]);
        }
    });
}

fn direction_toward_center(point: MapPoint) -> [f32; 2] {
    let x = -(point.x as f32);
    let z = -(point.z as f32);
    let length = (x * x + z * z).sqrt();
    if length == 0.0 {
        [0.0, 1.0]
    } else {
        [x / length, z / length]
    }
}
