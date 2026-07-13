use std::ops::Deref;

use hashbrown::HashMap;
use log::info;
use rapier3d::prelude::CollisionEvent;
use specs::{Entities, ReadExpect, ReadStorage, System, WriteExpect, WriteStorage};

use crate::{
    world::{
        components::{CurrentChunkComp, PositionComp, RigidBodyComp},
        interests::ChunkInterests,
        physics::Physics,
        registry::Registry,
        stats::Stats,
        voxels::Chunks,
        WorldConfig,
    },
    ClientFilter, ClientFlag, CollisionsComp, Event, EventBuilder, Events, IDComp, InteractorComp,
    Vec2, Vec3,
};

#[derive(Default)]
pub struct PhysicsSystem;

impl<'a> System<'a> for PhysicsSystem {
    type SystemData = (
        Entities<'a>,
        ReadExpect<'a, Stats>,
        ReadExpect<'a, Registry>,
        ReadExpect<'a, WorldConfig>,
        ReadExpect<'a, Chunks>,
        ReadExpect<'a, ChunkInterests>,
        WriteExpect<'a, Physics>,
        WriteExpect<'a, Events>,
        ReadStorage<'a, IDComp>,
        ReadStorage<'a, CurrentChunkComp>,
        ReadStorage<'a, InteractorComp>,
        ReadStorage<'a, ClientFlag>,
        WriteStorage<'a, CollisionsComp>,
        WriteStorage<'a, RigidBodyComp>,
        WriteStorage<'a, PositionComp>,
    );

    fn run(&mut self, data: Self::SystemData) {
        use rayon::prelude::*;
        use specs::{Join, ParJoin};

        let (
            entities,
            stats,
            registry,
            config,
            chunks,
            interests,
            mut physics,
            mut events,
            ids,
            curr_chunks,
            interactors,
            client_flag,
            mut collisions,
            mut bodies,
            mut positions,
        ) = data;

        if stats.preloading {
            return;
        }

        let mut collision_map = HashMap::new();

        // Tick the voxel physics of all entities (non-clients).
        // Skip entities in chunks with no interested players.
        (&curr_chunks, &mut bodies, &mut positions, !&client_flag)
            .par_join()
            .for_each(|(curr_chunk, body, position, _)| {
                if !chunks.is_chunk_ready(&curr_chunk.coords) {
                    return;
                }

                let cx = curr_chunk.coords.0;
                let cz = curr_chunk.coords.1;
                let mut neighbors_ready = true;
                for dx in -1i32..=1 {
                    for dz in -1i32..=1 {
                        if dx == 0 && dz == 0 {
                            continue;
                        }
                        let n = Vec2(cx + dx, cz + dz);
                        if chunks.is_within_world(&n) && !chunks.is_chunk_ready(&n) {
                            neighbors_ready = false;
                            break;
                        }
                    }
                    if !neighbors_ready {
                        break;
                    }
                }
                if !neighbors_ready {
                    body.0.forces.set(0.0, 0.0, 0.0);
                    body.0.impulses.set(0.0, 0.0, 0.0);
                    return;
                }

                if !interests.has_interests_in_region(&curr_chunk.coords) {
                    body.0.forces.set(0.0, 0.0, 0.0);
                    body.0.impulses.set(0.0, 0.0, 0.0);
                    return;
                }

                Physics::iterate_body(&mut body.0, stats.delta, chunks.deref(), &registry, &config);

                let body_pos = body.0.get_position();
                let Vec3(px, py, pz) = body_pos;
                position.0.set(px, py, pz);
            });

        // Move the clients' rigid bodies to their positions
        (&entities, &interactors, &bodies)
            .join()
            .for_each(|(ent, interactor, body)| {
                physics.move_rapier_body(interactor.body_handle(), &body.0.get_position());
                collision_map.insert(interactor.collider_handle().clone(), ent);
            });

        // Tick the rapier physics engine, and add the collisions to individual entities.
        let collision_events = physics.step(stats.delta);

        let mut started_collisions = Vec::new();
        let mut stopped_collisions = Vec::new();

        for event in collision_events {
            match event {
                CollisionEvent::Started(ch1, ch2, _) => {
                    if let (Some(ent1), Some(ent2)) =
                        (collision_map.get(&ch1), collision_map.get(&ch2))
                    {
                        started_collisions.push((*ent1, *ent2, event));
                    }
                }
                CollisionEvent::Stopped(ch1, ch2, _) => {
                    if let (Some(ent1), Some(ent2)) =
                        (collision_map.get(&ch1), collision_map.get(&ch2))
                    {
                        stopped_collisions.push((*ent1, *ent2, event));
                    }
                }
                _ => {}
            }
        }

        for (ent1, ent2, event) in started_collisions {
            if let Some(collision_comp) = collisions.get_mut(ent1) {
                collision_comp.0.push((event, ent2));
            }
            if let Some(collision_comp) = collisions.get_mut(ent2) {
                collision_comp.0.push((event, ent1));
            }
        }

        for (ent1, ent2, event) in stopped_collisions {
            if let Some(collision_comp) = collisions.get_mut(ent1) {
                collision_comp.0.push((event, ent2));
            }
            if let Some(collision_comp) = collisions.get_mut(ent2) {
                collision_comp.0.push((event, ent1));
            }
        }

        if config.collision_repulsion <= f32::EPSILON {
            return;
        }

        // Collision detection, push bodies away from one another.
        let mut collision_data = Vec::new();
        for (curr_chunk, body, interactor, entity) in
            (&curr_chunks, &mut bodies, &interactors, &entities).join()
        {
            if !chunks.is_chunk_ready(&curr_chunk.coords) {
                continue;
            }

            let rapier_body = physics.get(&interactor.0);
            let after = rapier_body.translation();
            // PositionComp 对玩家表示眼睛位置，Rapier 与 RigidBodyComp 均表示碰撞体中心。
            let Vec3(dx, dy, dz) =
                rapier_displacement_from_body_center(&body.0, [after.x, after.y, after.z]);

            let dx = if dx.abs() < 0.001 { 0.0 } else { dx };
            let dy = if dy.abs() < 0.001 { 0.0 } else { dy };
            let dz = if dz.abs() < 0.001 { 0.0 } else { dz };

            let len = (dx * dx + dy * dy + dz * dz).sqrt();

            if len > 0.0001 {
                collision_data.push((body, dx, dy, dz, len, entity));
            }
        }
        for (body, dx, dy, dz, len, entity) in collision_data {
            let mut dx = dx / len;
            let dy = dy / len;
            let mut dz = dz / len;

            // If only dy movements, add a little bias to eliminate stack overflow.
            if dx.abs() < 0.001 && dz.abs() < 0.001 {
                dx = fastrand::i32(-10..10) as f32 / 1000.0;
                dz = fastrand::i32(-10..10) as f32 / 1000.0;
            }

            // Check if the entity is a client, and if so, apply the impulse to the client's body.
            if client_flag.get(entity).is_some() {
                if let Some(id) = ids.get(entity) {
                    let event = EventBuilder::new("vox-builtin:impulse")
                        .payload(vec![
                            dx * config.client_collision_repulsion,
                            dy * config.client_collision_repulsion,
                            dz * config.client_collision_repulsion,
                        ])
                        .filter(ClientFilter::Direct(id.0.to_owned()))
                        .build();
                    events.dispatch(event);
                    continue;
                }
            }

            // Apply the impulse to the body.
            body.0.apply_impulse(
                (dx * config.collision_repulsion).min(3.0),
                (dy * config.collision_repulsion).min(3.0),
                (dz * config.collision_repulsion).min(3.0),
            );
        }
    }
}

fn rapier_displacement_from_body_center(body: &crate::RigidBody, after: [f32; 3]) -> Vec3<f32> {
    let Vec3(px, py, pz) = body.get_position();
    Vec3(after[0] - px, after[1] - py, after[2] - pz)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AABB;

    #[test]
    fn rapier_displacement_uses_body_center_instead_of_client_eye_position() {
        let mut body =
            crate::RigidBody::new(&AABB::new().scale_x(0.8).scale_y(1.8).scale_z(0.8).build())
                .build();
        body.set_position(1.0, 2.0, 3.0);

        let Vec3(dx, dy, dz) = rapier_displacement_from_body_center(&body, [1.0, 2.0, 3.0]);
        assert_eq!([dx, dy, dz], [0.0, 0.0, 0.0]);
    }
}
