use super::*;
use crate::{ChunkProtocol, GeometryProtocol, MeshProtocol, UpdateProtocol};
use hashbrown::HashMap;
use specs::{Builder, RunNow, WorldExt};

const DIRT: u32 = 1;
const GOLD: u32 = 2;
const DIAMOND: u32 = 3;
const GLASS: u32 = 4;

fn registry() -> Registry {
    let mut registry = Registry::new();
    for (id, name) in [(DIRT, "Dirt"), (GOLD, "Gold"), (DIAMOND, "Diamond")] {
        registry.register_block(&Block::new(name).id(id).build());
    }
    registry.register_block(
        &Block::new("Glass")
            .id(GLASS)
            .is_x_transparent(true)
            .is_y_transparent(true)
            .is_z_transparent(true)
            .build(),
    );
    registry
}

fn config() -> WorldConfig {
    config_with_client_only_meshing(true)
}

fn config_with_client_only_meshing(client_only_meshing: bool) -> WorldConfig {
    WorldConfig::new()
        .chunk_size(2)
        .sub_chunks(2)
        .max_height(4)
        .min_chunk([0, 0])
        .max_chunk([1, 0])
        .client_only_meshing(client_only_meshing)
        .build()
}

fn ready_chunks() -> Chunks {
    let config = config();
    let options = ChunkOptions {
        size: config.chunk_size,
        max_height: config.max_height,
        sub_chunks: config.sub_chunks,
    };
    let mut chunks = Chunks::new(&config);
    for cx in 0..=1 {
        let mut chunk = Chunk::new(&format!("chunk-{cx}"), cx, 0, &options);
        for x in chunk.min.0..chunk.max.0 {
            for y in 0..config.max_height as i32 {
                for z in chunk.min.2..chunk.max.2 {
                    chunk.set_voxel(x, y, z, DIRT);
                }
            }
        }
        chunk.status = ChunkStatus::Ready;
        chunks.add(chunk);
    }
    chunks
}

fn model_voxel(model: &ChunkProtocol, position: Vec3<i32>) -> u32 {
    let voxels = model.voxels.as_ref().unwrap();
    let lx = position.0.rem_euclid(2) as usize;
    let lz = position.2.rem_euclid(2) as usize;
    BlockUtils::extract_id(voxels[&[lx, position.1 as usize, lz]])
}

fn load_message_voxel(message: &Message, index: usize) -> u32 {
    let chunk = message.chunks.first().unwrap();
    let bytes = lz4_flex::block::decompress_size_prepended(&chunk.voxels).unwrap();
    let offset = index * std::mem::size_of::<u32>();
    u32::from_ne_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

#[test]
fn default_projection_keeps_legacy_chunk_data_identical() {
    let registry = registry();
    let mut chunks = ready_chunks();
    chunks.set_voxel(0, 1, 0, GOLD);
    let chunk = chunks.get(&Vec2(0, 0)).unwrap();
    let expected = chunk.to_model(false, true, 0..2);

    let actual = ChunkProjection::default().project_chunk(chunk, &chunks, &registry, false, 0..2);

    assert_eq!(actual.voxels.unwrap().data, expected.voxels.unwrap().data);
}

#[test]
fn opt_in_projection_hides_enclosed_ore_but_keeps_server_truth() {
    let registry = registry();
    let mut chunks = ready_chunks();
    let position = Vec3(0, 1, 0);
    chunks.set_voxel(position.0, position.1, position.2, GOLD);
    let chunk = chunks.get(&Vec2(0, 0)).unwrap();
    let mut projection = ChunkProjection::obfuscating([(GOLD, DIRT), (DIAMOND, DIRT)]).unwrap();

    let model = projection.project_chunk(chunk, &chunks, &registry, false, 0..2);

    assert_eq!(model_voxel(&model, position.clone()), DIRT);
    assert_eq!(chunks.get_voxel(position.0, position.1, position.2), GOLD);
    assert!(!projection.is_revealed(&position));
}

#[test]
fn transparent_cross_chunk_neighbor_is_visible_on_load() {
    let registry = registry();
    let mut chunks = ready_chunks();
    let ore = Vec3(1, 1, 0);
    chunks.set_voxel(ore.0, ore.1, ore.2, DIAMOND);
    chunks.set_voxel(2, 1, 0, GLASS);
    let chunk = chunks.get(&Vec2(0, 0)).unwrap();
    let mut projection = ChunkProjection::obfuscating([(DIAMOND, DIRT)]).unwrap();

    let model = projection.project_chunk(chunk, &chunks, &registry, false, 0..2);

    assert_eq!(model_voxel(&model, ore.clone()), DIAMOND);
    assert!(projection.is_revealed(&ore));
}

#[test]
fn authoritative_air_update_reveals_cross_chunk_ore_once() {
    let registry = registry();
    let mut chunks = ready_chunks();
    let air = Vec3(1, 1, 0);
    let ore = Vec3(2, 1, 0);
    chunks.set_voxel(ore.0, ore.1, ore.2, GOLD);
    chunks.set_voxel(air.0, air.1, air.2, 0);
    let mut projection = ChunkProjection::obfuscating([(GOLD, DIRT)]).unwrap();
    let air_update = UpdateProtocol {
        vx: air.0,
        vy: air.1,
        vz: air.2,
        voxel: chunks.get_raw_voxel(air.0, air.1, air.2),
        light: 0,
    };

    let first = projection.project_updates(
        vec![air_update.clone()],
        &chunks,
        &registry,
        config().chunk_size,
        config().max_height as i32,
    );
    let second = projection.project_updates(
        vec![air_update],
        &chunks,
        &registry,
        config().chunk_size,
        config().max_height as i32,
    );

    assert_eq!(first.len(), 2);
    assert!(first.iter().any(|update| {
        update.vx == ore.0
            && update.vy == ore.1
            && update.vz == ore.2
            && BlockUtils::extract_id(update.voxel) == GOLD
    }));
    assert_eq!(second.len(), 1);
    assert_eq!(chunks.get_voxel(ore.0, ore.1, ore.2), GOLD);
    assert!(projection.is_revealed(&ore));
}

#[test]
fn stale_air_update_cannot_reveal_ore_after_final_opaque_update() {
    let registry = registry();
    let mut chunks = ready_chunks();
    let changed = Vec3(1, 1, 0);
    let ore = Vec3(2, 1, 0);
    chunks.set_voxel(ore.0, ore.1, ore.2, GOLD);
    chunks.set_voxel(changed.0, changed.1, changed.2, DIRT);
    let mut projection = ChunkProjection::obfuscating([(GOLD, DIRT)]).unwrap();
    let updates = vec![
        UpdateProtocol {
            vx: changed.0,
            vy: changed.1,
            vz: changed.2,
            voxel: 0,
            light: 0,
        },
        UpdateProtocol {
            vx: changed.0,
            vy: changed.1,
            vz: changed.2,
            voxel: DIRT,
            light: 0,
        },
    ];

    let projected = projection.project_updates(
        updates,
        &chunks,
        &registry,
        config().chunk_size,
        config().max_height as i32,
    );

    assert!(!projection.is_revealed(&ore));
    assert!(!projected
        .iter()
        .any(|update| Vec3(update.vx, update.vy, update.vz) == ore));
}

#[test]
fn opt_in_update_only_reaches_clients_interested_in_its_chunk() {
    let config = config();
    let mut world = World::new("projected-update-routing", &config);
    world.ecs_mut().insert(registry());
    world
        .ecs_mut()
        .insert(ChunkProjection::obfuscating([(GOLD, DIRT)]).unwrap());
    {
        let mut chunks = world.chunks_mut();
        *chunks = ready_chunks();
        chunks.set_voxel(2, 1, 0, GOLD);
        chunks.update_voxel(&Vec3(1, 1, 0), 0);
    }
    {
        let mut interests = world.chunk_interest_mut();
        interests.add("air-viewer", &Vec2(0, 0));
        interests.add("ore-viewer", &Vec2(1, 0));
    }

    let mut system = ChunkUpdatingSystem;
    system.run_now(world.ecs_mut());
    world.ecs_mut().maintain();
    let queued = world.write_resource::<MessageQueues>().drain_prioritized();

    assert!(!queued.is_empty());
    assert!(queued
        .iter()
        .all(|(_, filter)| matches!(filter, ClientFilter::Direct(_))));
    let ore_message = queued
        .iter()
        .find(|(_, filter)| {
            matches!(filter, ClientFilter::Direct(client_id) if client_id == "ore-viewer")
        })
        .unwrap();
    assert!(ore_message.0.updates.iter().any(|update| {
        update.vx == 2 && update.vy == 1 && update.vz == 0 && update.voxel == GOLD
    }));
    let air_message = queued
        .iter()
        .find(|(_, filter)| {
            matches!(filter, ClientFilter::Direct(client_id) if client_id == "air-viewer")
        })
        .unwrap();
    assert!(!air_message
        .0
        .updates
        .iter()
        .any(|update| update.vx == 2 && update.vy == 1 && update.vz == 0));
}

#[test]
fn both_ready_and_new_chunk_load_paths_emit_projected_data() {
    let config = config_with_client_only_meshing(false);
    let mut world = World::new("projected-load-paths", &config);
    world.ecs_mut().insert(registry());
    world
        .ecs_mut()
        .insert(ChunkProjection::obfuscating([(GOLD, DIRT)]).unwrap());
    {
        let mut chunks = world.chunks_mut();
        *chunks = ready_chunks();
        chunks.set_voxel(0, 1, 0, GOLD);
        chunks.get_mut(&Vec2(0, 0)).unwrap().meshes = Some(HashMap::from([(
            0,
            MeshProtocol {
                level: 0,
                geometries: vec![GeometryProtocol {
                    voxel: GOLD,
                    ..Default::default()
                }],
            },
        )]));
    }
    let mut requests = ChunkRequestsComp::new();
    requests.add(&Vec2(0, 0));
    world
        .ecs_mut()
        .create_entity()
        .with(IDComp::new("client"))
        .with(requests)
        .build();

    let mut requests_system = ChunkRequestsSystem;
    requests_system.run_now(world.ecs_mut());
    let ready_messages = world.write_resource::<MessageQueues>().drain_prioritized();
    let ready_load = ready_messages
        .iter()
        .find(|(message, _)| MessageType::try_from(message.r#type) == Ok(MessageType::Load))
        .unwrap();
    assert!(ready_load
        .0
        .chunks
        .iter()
        .all(|chunk| chunk.meshes.is_empty()));
    assert_eq!(load_message_voxel(&ready_load.0, 2), DIRT);

    world
        .chunks_mut()
        .add_chunk_to_send(&Vec2(0, 0), &MessageType::Load, false);
    let mut sending_system = ChunkSendingSystem::new();
    sending_system.run_now(world.ecs_mut());
    let generated_messages = world.write_resource::<MessageQueues>().drain_prioritized();
    let generated_load = generated_messages
        .iter()
        .find(|(message, _)| MessageType::try_from(message.r#type) == Ok(MessageType::Load))
        .unwrap();
    assert!(generated_load
        .0
        .chunks
        .iter()
        .all(|chunk| chunk.meshes.is_empty()));
    assert_eq!(load_message_voxel(&generated_load.0, 2), DIRT);

    world
        .chunks_mut()
        .add_chunk_to_send(&Vec2(0, 0), &MessageType::Update, false);
    sending_system.run_now(world.ecs_mut());
    let update_messages = world.write_resource::<MessageQueues>().drain_prioritized();
    assert!(update_messages
        .iter()
        .any(|(message, _)| { MessageType::try_from(message.r#type) == Ok(MessageType::Update) }));
    assert!(update_messages
        .iter()
        .flat_map(|(message, _)| &message.chunks)
        .all(|chunk| chunk.meshes.is_empty()));
    assert_eq!(world.chunks().get_voxel(0, 1, 0), GOLD);
}
