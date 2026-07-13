use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};

use crate::{EventProtocol, UpdateProtocol};
use futures_util::future::join_all;

use super::*;

fn ws_sender() -> (WsSender, crate::server::WsReceiver) {
    WsSender::channel(64)
}

fn test_world(name: &str, config: &WorldConfig) -> World {
    let mut registry = Registry::new();
    registry.generate();
    let mut world = World::new(name, config);
    world.ecs_mut().insert(registry);
    world
}

#[actix::test]
async fn concurrent_join_enforces_world_capacity_atomically() {
    let config = WorldConfig::new().max_clients(10).build();
    let address = test_world("capacity", &config).start();
    address.send(Prepare).await.unwrap();

    let mut receivers = Vec::new();
    let mut requests = Vec::new();
    for index in 0..11 {
        let (sender, receiver) = ws_sender();
        receivers.push(receiver);
        requests.push(address.send(ClientJoinRequest {
            id: format!("player-{index}"),
            username: format!("Player {index}"),
            sender,
            preferences: ClientPreferencesPatch::default(),
            principal: Some(ConnectionPrincipal::new(
                format!("account-{index}"),
                format!("session-{index}"),
            )),
            join_attempt_id: format!("attempt-{index}"),
        }));
    }

    let results = join_all(requests).await;
    let accepted = results
        .iter()
        .filter(|result| matches!(result, Ok(Ok(_))))
        .count();
    let rejected = results
        .iter()
        .filter(|result| matches!(result, Ok(Err(ClientJoinError::WorldFull { capacity: 10 }))))
        .count();
    let stats = address.send(GetWorldStats).await.unwrap();

    assert_eq!(accepted, 10);
    assert_eq!(rejected, 1);
    assert_eq!(stats.client_count, 10);
    let _ = address.send(StopWorld).await;
    drop(receivers);
}

#[test]
fn detached_client_rebinds_only_with_the_same_principal() {
    let config = WorldConfig::new()
        .client_disconnect_policy(ClientDisconnectPolicy::Detach)
        .build();
    let mut world = test_world("rebind", &config);
    world.prepare();
    let owner = ConnectionPrincipal::new("account-1", "session-1");
    let (sender, _receiver) = ws_sender();
    world
        .add_client(
            "player-1",
            "Player",
            &sender,
            ClientPreferencesPatch::default(),
            Some(owner.clone()),
            "attempt-owner".to_owned(),
        )
        .unwrap();

    assert_eq!(
        world.detach_client("player-1"),
        ClientDetachOutcome::Detached
    );
    let (attacker_sender, _attacker_receiver) = ws_sender();
    assert_eq!(
        world.rebind_client(
            "player-1",
            &attacker_sender,
            &ConnectionPrincipal::new("account-2", "session-2"),
            "attacker-attempt".to_owned(),
        ),
        Err(ClientRebindError::PrincipalMismatch)
    );

    let (new_sender, _new_receiver) = ws_sender();
    assert!(world
        .rebind_client(
            "player-1",
            &new_sender,
            &ConnectionPrincipal::new("account-1", "session-2"),
            "owner-rebind-attempt".to_owned(),
        )
        .is_ok());
    assert_eq!(world.clients().len(), 1);
    assert!(world.clients().get("player-1").unwrap().attached);
}

#[test]
fn stale_join_cleanup_cannot_remove_a_newer_client_lease() {
    let config = WorldConfig::default();
    let mut world = test_world("join-lease", &config);
    world.prepare();
    let (sender, _receiver) = ws_sender();
    world
        .add_client(
            "player-1",
            "Player",
            &sender,
            ClientPreferencesPatch::default(),
            None,
            "current-attempt".to_owned(),
        )
        .unwrap();

    assert!(!world.remove_client_for_join_attempt("player-1", "stale-attempt"));
    assert_eq!(world.clients().len(), 1);
    assert!(world.remove_client_for_join_attempt("player-1", "current-attempt"));
    assert!(world.clients().is_empty());
}

#[test]
fn stale_rebind_cleanup_cannot_remove_a_newer_attach_lease() {
    let config = WorldConfig::new()
        .client_disconnect_policy(ClientDisconnectPolicy::Detach)
        .build();
    let mut world = test_world("rebind-lease", &config);
    world.prepare();
    let principal = ConnectionPrincipal::new("account-1", "session-1");
    let (sender, _receiver) = ws_sender();
    world
        .add_client(
            "player-1",
            "Player",
            &sender,
            ClientPreferencesPatch::default(),
            Some(principal.clone()),
            "join-attempt".to_owned(),
        )
        .unwrap();
    assert_eq!(
        world.detach_client("player-1"),
        ClientDetachOutcome::Detached
    );
    let (first_sender, _first_receiver) = ws_sender();
    world
        .rebind_client(
            "player-1",
            &first_sender,
            &principal,
            "rebind-attempt-1".to_owned(),
        )
        .unwrap();
    assert_eq!(
        world.detach_client("player-1"),
        ClientDetachOutcome::Detached
    );
    let (second_sender, _second_receiver) = ws_sender();
    world
        .rebind_client(
            "player-1",
            &second_sender,
            &principal,
            "rebind-attempt-2".to_owned(),
        )
        .unwrap();

    assert!(!world.remove_client_for_join_attempt("player-1", "rebind-attempt-1"));
    assert!(world.remove_client_for_join_attempt("player-1", "rebind-attempt-2"));
}

#[test]
fn attach_guard_denies_join_before_world_state_is_written() {
    let mut world = test_world("guarded-join", &WorldConfig::default());
    world.prepare();
    world.set_client_attach_guard(|request| {
        assert_eq!(request.kind, ClientAttachKind::Join);
        assert_eq!(request.world_name, "guarded-join");
        assert_eq!(request.client_id, "player-1");
        assert_eq!(request.attach_attempt_id, "guarded-attempt");
        false
    });
    let (sender, _receiver) = ws_sender();

    let result = world.add_client(
        "player-1",
        "Player",
        &sender,
        ClientPreferencesPatch::default(),
        Some(ConnectionPrincipal::new("account-1", "session-1")),
        "guarded-attempt".to_owned(),
    );

    assert_eq!(result, Err(ClientJoinError::AdmissionDenied));
    assert!(world.clients().is_empty());
    assert!(world.entity_ids().get("player-1").is_none());
}

#[test]
fn attach_guard_denies_rebind_before_address_is_restored() {
    let config = WorldConfig::new()
        .client_disconnect_policy(ClientDisconnectPolicy::Detach)
        .build();
    let mut world = test_world("guarded-rebind", &config);
    world.prepare();
    world.set_client_attach_guard(|request| {
        if request.kind == ClientAttachKind::Rebind {
            assert_eq!(request.attach_attempt_id, "guarded-rebind-retry");
            return false;
        }
        true
    });
    let principal = ConnectionPrincipal::new("account-1", "session-1");
    let (sender, _receiver) = ws_sender();
    world
        .add_client(
            "player-1",
            "Player",
            &sender,
            ClientPreferencesPatch::default(),
            Some(principal.clone()),
            "guarded-rebind-attempt".to_owned(),
        )
        .unwrap();
    assert_eq!(
        world.detach_client("player-1"),
        ClientDetachOutcome::Detached
    );
    let entity = world.clients().get("player-1").unwrap().entity;
    assert!(world.read_component::<AddrComp>().get(entity).is_none());
    let (replacement, _replacement_receiver) = ws_sender();

    let result = world.rebind_client(
        "player-1",
        &replacement,
        &principal,
        "guarded-rebind-retry".to_owned(),
    );

    assert_eq!(result, Err(ClientRebindError::AdmissionDenied));
    assert!(!world.clients().get("player-1").unwrap().attached);
    assert!(world.read_component::<AddrComp>().get(entity).is_none());
}

#[test]
fn preload_prepares_created_world_and_is_idempotent() {
    let config = WorldConfig::new().preload(true).preload_radius(0).build();
    let mut world = test_world("preload-lifecycle", &config);

    world.preload();
    assert_eq!(world.lifecycle, WorldLifecycleState::Preparing);
    assert!(world.preloading);

    world.preload();
    assert_eq!(world.lifecycle, WorldLifecycleState::Preparing);
    assert!(world.preloading);
}

#[test]
fn prepared_world_ticks_without_dispatcher_dependency_panic() {
    let config = WorldConfig::default();
    let mut world = test_world("first-tick", &config);
    world.prepare();

    world.tick();

    assert!(world.started);
    assert_eq!(world.lifecycle, WorldLifecycleState::Ready);
}

struct CountingSystem(Arc<AtomicUsize>);

impl<'a> specs::System<'a> for CountingSystem {
    type SystemData = ();

    fn run(&mut self, _: Self::SystemData) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct OrderedSystem(&'static str, Arc<Mutex<Vec<&'static str>>>);

impl<'a> specs::System<'a> for OrderedSystem {
    type SystemData = ();

    fn run(&mut self, _: Self::SystemData) {
        self.1.lock().unwrap().push(self.0);
    }
}

struct QueueAir(Vec3<i32>);

impl<'a> specs::System<'a> for QueueAir {
    type SystemData = specs::WriteExpect<'a, Chunks>;

    fn run(&mut self, mut chunks: Self::SystemData) {
        chunks.update_voxel(&self.0, 0);
    }
}

struct ObserveVoxel(Vec3<i32>, Arc<AtomicUsize>);

impl<'a> specs::System<'a> for ObserveVoxel {
    type SystemData = specs::ReadExpect<'a, Chunks>;

    fn run(&mut self, chunks: Self::SystemData) {
        self.1.store(
            chunks.get_voxel(self.0 .0, self.0 .1, self.0 .2) as usize,
            Ordering::SeqCst,
        );
    }
}

#[test]
fn dispatcher_extension_preserves_the_current_factory() {
    let base_calls = Arc::new(AtomicUsize::new(0));
    let extension_calls = Arc::new(AtomicUsize::new(0));
    let mut world = test_world("dispatcher-extension", &WorldConfig::default());

    let base_calls_for_factory = base_calls.clone();
    world.set_dispatcher(move || {
        TimedDispatcherBuilder::new().with(
            CountingSystem(base_calls_for_factory.clone()),
            "base-system",
            &[],
        )
    });
    world.prepare();
    world.tick();

    let extension_calls_for_factory = extension_calls.clone();
    world.extend_dispatcher(move |builder| {
        builder.with(
            CountingSystem(extension_calls_for_factory.clone()),
            "extension-system",
            &["base-system"],
        )
    });
    world.tick();

    assert_eq!(base_calls.load(Ordering::SeqCst), 2);
    assert_eq!(extension_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn before_spatial_hook_precedes_position_consumers_and_gameplay() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut world = test_world("before-spatial-hook", &WorldConfig::default());

    let movement_calls = calls.clone();
    world
        .install_before_spatial_update_system("authoritative-movement-test", move || {
            OrderedSystem("movement", movement_calls.clone())
        })
        .unwrap();
    let peer_calls = calls.clone();
    world.extend_dispatcher(move |builder| {
        builder.with(
            OrderedSystem("peer-metadata", peer_calls.clone()),
            "observe-after-peer-metadata",
            &["peers-meta"],
        )
    });
    let mining_calls = calls.clone();
    world
        .install_before_chunk_updating_system("mining-after-movement-test", move || {
            OrderedSystem("mining", mining_calls.clone())
        })
        .unwrap();
    let physics_calls = calls.clone();
    world.extend_dispatcher(move |builder| {
        builder.with(
            OrderedSystem("physics-consumer", physics_calls.clone()),
            "observe-after-physics",
            &["physics"],
        )
    });

    world.prepare();
    world.tick();

    let calls = calls.lock().unwrap();
    let movement = calls.iter().position(|call| *call == "movement").unwrap();
    let peer_metadata = calls
        .iter()
        .position(|call| *call == "peer-metadata")
        .unwrap();
    let mining = calls.iter().position(|call| *call == "mining").unwrap();
    let physics = calls
        .iter()
        .position(|call| *call == "physics-consumer")
        .unwrap();
    assert!(movement < peer_metadata);
    assert!(movement < mining);
    assert!(mining < physics);
}

#[test]
fn before_spatial_hook_rejects_conflicts_and_custom_dispatchers() {
    let mut world = test_world("before-spatial-conflicts", &WorldConfig::default());
    assert_eq!(
        world.install_before_spatial_update_system("current-chunk", || CountingSystem(Arc::new(
            AtomicUsize::new(0)
        ))),
        Err(DispatcherHookError::NameConflict)
    );
    world
        .install_before_spatial_update_system("application-before-spatial", || {
            CountingSystem(Arc::new(AtomicUsize::new(0)))
        })
        .unwrap();
    assert_eq!(
        world.install_before_spatial_update_system("second-spatial-hook", || CountingSystem(
            Arc::new(AtomicUsize::new(0))
        )),
        Err(DispatcherHookError::HookAlreadyInstalled)
    );

    let mut custom = test_world("custom-before-spatial", &WorldConfig::default());
    custom.set_dispatcher(TimedDispatcherBuilder::new);
    assert_eq!(
        custom.install_before_spatial_update_system("application-before-spatial", || {
            CountingSystem(Arc::new(AtomicUsize::new(0)))
        }),
        Err(DispatcherHookError::CustomDispatcherUnsupported)
    );
}

#[test]
fn before_chunk_hook_rebuilds_cache_and_preserves_post_extensions() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut world = test_world("before-chunk-hook", &WorldConfig::default());
    let after_calls = calls.clone();
    world.extend_dispatcher(move |builder| {
        builder.with(
            OrderedSystem("after", after_calls.clone()),
            "after-chunk-test",
            &["chunk-updating"],
        )
    });
    world.prepare();
    world.tick();
    calls.lock().unwrap().clear();

    let before_calls = calls.clone();
    world
        .install_before_chunk_updating_system("before-chunk-test", move || {
            OrderedSystem("before", before_calls.clone())
        })
        .unwrap();
    world.tick();

    assert_eq!(*calls.lock().unwrap(), vec!["before", "after"]);
}

#[test]
fn before_chunk_hook_flushes_staged_voxel_in_the_same_tick() {
    let config = WorldConfig::new()
        .min_chunk([0, 0])
        .max_chunk([0, 0])
        .max_height(16)
        .max_light_level(1)
        .build();
    let mut registry = Registry::new();
    registry.register_block(&Block::new("Test Solid").id(1).build());
    registry.generate();
    let mut world = World::new("before-chunk-voxel", &config);
    world.ecs_mut().insert(registry);

    let target = Vec3(1, 1, 1);
    let mut chunk = Chunk::new(
        "ready-0-0",
        0,
        0,
        &ChunkOptions {
            size: config.chunk_size,
            max_height: config.max_height,
            sub_chunks: config.sub_chunks,
        },
    );
    assert!(chunk.set_voxel(target.0, target.1, target.2, 1));
    chunk.status = ChunkStatus::Ready;
    world.chunks_mut().add(chunk);

    let observed = Arc::new(AtomicUsize::new(usize::MAX));
    let observed_by_system = observed.clone();
    let observed_target = target.clone();
    world.extend_dispatcher(move |builder| {
        builder.with(
            ObserveVoxel(observed_target.clone(), observed_by_system.clone()),
            "observe-voxel-after-chunk-update",
            &["chunk-updating"],
        )
    });
    world.prepare();
    world.tick();
    assert_eq!(observed.load(Ordering::SeqCst), 1);

    world
        .install_before_chunk_updating_system("queue-air-before-chunk-update", move || {
            QueueAir(target.clone())
        })
        .unwrap();
    world.tick();

    assert_eq!(observed.load(Ordering::SeqCst), 0);
    assert!(world.chunks().updates_staging.is_empty());
}

#[test]
fn before_chunk_hook_rejects_conflicts_and_custom_dispatchers() {
    let mut world = test_world("before-chunk-conflicts", &WorldConfig::default());
    assert_eq!(
        world.install_before_chunk_updating_system("chunk-updating", || CountingSystem(Arc::new(
            AtomicUsize::new(0)
        ))),
        Err(DispatcherHookError::NameConflict)
    );
    world
        .install_before_chunk_updating_system("application-before-chunk", || {
            CountingSystem(Arc::new(AtomicUsize::new(0)))
        })
        .unwrap();
    assert_eq!(
        world.install_before_chunk_updating_system("second-hook", || CountingSystem(Arc::new(
            AtomicUsize::new(0)
        ))),
        Err(DispatcherHookError::HookAlreadyInstalled)
    );

    let mut custom = test_world("custom-before-chunk", &WorldConfig::default());
    custom.set_dispatcher(TimedDispatcherBuilder::new);
    assert_eq!(
        custom.install_before_chunk_updating_system("application-before-chunk", || {
            CountingSystem(Arc::new(AtomicUsize::new(0)))
        }),
        Err(DispatcherHookError::CustomDispatcherUnsupported)
    );
}

#[test]
fn before_broadcast_hook_runs_before_post_broadcast_extensions() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut world = test_world("before-broadcast-hook", &WorldConfig::default());
    let before_calls = calls.clone();
    world
        .install_before_broadcast_system("application-before-broadcast", move || {
            OrderedSystem("before", before_calls.clone())
        })
        .unwrap();
    let after_calls = calls.clone();
    world.extend_dispatcher(move |builder| {
        builder.with(
            OrderedSystem("after", after_calls.clone()),
            "after-broadcast-test",
            &["broadcast"],
        )
    });

    world.prepare();
    world.tick();

    assert_eq!(*calls.lock().unwrap(), vec!["before", "after"]);
}

#[test]
fn before_broadcast_hook_rejects_conflicts_and_custom_dispatchers() {
    let mut world = test_world("before-broadcast-conflicts", &WorldConfig::default());
    assert_eq!(
        world.install_before_broadcast_system("broadcast", || CountingSystem(Arc::new(
            AtomicUsize::new(0)
        ))),
        Err(DispatcherHookError::NameConflict)
    );
    world
        .install_before_broadcast_system("application-before-broadcast", || {
            CountingSystem(Arc::new(AtomicUsize::new(0)))
        })
        .unwrap();
    assert_eq!(
        world.install_before_broadcast_system("second-broadcast-hook", || CountingSystem(
            Arc::new(AtomicUsize::new(0))
        )),
        Err(DispatcherHookError::HookAlreadyInstalled)
    );

    let mut custom = test_world("custom-before-broadcast", &WorldConfig::default());
    custom.set_dispatcher(TimedDispatcherBuilder::new);
    assert_eq!(
        custom.install_before_broadcast_system("application-before-broadcast", || {
            CountingSystem(Arc::new(AtomicUsize::new(0)))
        }),
        Err(DispatcherHookError::CustomDispatcherUnsupported)
    );
}

#[test]
fn client_modifier_extension_runs_after_the_existing_modifier() {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let mut world = test_world("client-modifier-extension", &WorldConfig::default());

    let first_calls = calls.clone();
    world.set_client_modifier(move |_, _| first_calls.lock().unwrap().push("first"));
    let second_calls = calls.clone();
    world.add_client_modifier(move |_, _| second_calls.lock().unwrap().push("second"));
    world.prepare();
    let (sender, _receiver) = ws_sender();

    world
        .add_client(
            "player-1",
            "Player",
            &sender,
            ClientPreferencesPatch::default(),
            None,
            "modifier-attempt".to_owned(),
        )
        .unwrap();

    assert_eq!(*calls.lock().unwrap(), vec!["first", "second"]);
}

#[test]
fn strict_policy_blocks_unlisted_method_and_movement_flags() {
    let calls = Arc::new(AtomicUsize::new(0));
    let config = WorldConfig::new()
        .request_policy(WorldRequestPolicy::strict())
        .build();
    let mut world = test_world("strict", &config);
    world.prepare();
    let calls_for_handler = calls.clone();
    world.set_method_handle("pvp:v1:attack", move |_, _, _| {
        calls_for_handler.fetch_add(1, Ordering::SeqCst);
    });
    let (sender, _receiver) = ws_sender();
    world
        .add_client(
            "player-1",
            "Player",
            &sender,
            ClientPreferencesPatch::default(),
            None,
            "attempt-strict".to_owned(),
        )
        .unwrap();

    world.on_request(
        "player-1",
        Message::new(&MessageType::Method)
            .method(MethodProtocol {
                name: "PVP:V1:ATTACK".to_owned(),
                payload: "{}".to_owned(),
            })
            .build(),
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let entity = world.clients().get("player-1").unwrap().entity;
    let gravity_before = world
        .read_component::<RigidBodyComp>()
        .get(entity)
        .unwrap()
        .0
        .gravity_multiplier;
    world.on_request(
        "player-1",
        Message::new(&MessageType::Peer)
            .peers(&[PeerProtocol {
                id: "player-1".to_owned(),
                username: "Player".to_owned(),
                metadata: r#"{"isFlying":true}"#.to_owned(),
            }])
            .build(),
    );
    let gravity_after = world
        .read_component::<RigidBodyComp>()
        .get(entity)
        .unwrap()
        .0
        .gravity_multiplier;
    assert_eq!(gravity_after, gravity_before);
}

#[test]
fn strict_policy_rejects_single_and_bulk_raw_voxel_updates_before_staging() {
    let config = WorldConfig::new()
        .request_policy(WorldRequestPolicy::strict())
        .build();
    let mut world = test_world("strict-raw-updates", &config);
    world.prepare();
    let (sender, _receiver) = ws_sender();
    world
        .add_client(
            "player-1",
            "Player",
            &sender,
            ClientPreferencesPatch::default(),
            None,
            "strict-update-attempt".to_owned(),
        )
        .unwrap();

    world.on_request(
        "player-1",
        Message::new(&MessageType::Update)
            .updates(&[UpdateProtocol {
                vx: 1,
                vy: 1,
                vz: 1,
                voxel: 7,
                light: 0,
            }])
            .build(),
    );
    let mut bulk = Message::new(&MessageType::Update).build();
    bulk.bulk_update = Some(crate::protocols::BulkUpdate {
        vx: vec![2],
        vy: vec![2],
        vz: vec![2],
        voxels: vec![8],
        lights: vec![0],
    });
    world.on_request("player-1", bulk);

    assert!(world.chunks().updates_staging.is_empty());
}

#[test]
fn authoritative_chunk_load_ignores_client_center_and_rejects_far_or_outside_chunks() {
    let config = WorldConfig::new()
        .min_chunk([-10, -10])
        .max_chunk([9, 9])
        .chunk_load_policy(ChunkLoadPolicy::authoritative_radius(6))
        .build();
    let mut world = test_world("bounded-load", &config);
    world.set_client_modifier(|world, entity| {
        world
            .write_component::<PositionComp>()
            .insert(entity, PositionComp::new(-0.1, 20.0, -16.1))
            .unwrap();
        world
            .write_component::<DirectionComp>()
            .insert(entity, DirectionComp::new(0.0, 0.0, 1.0))
            .unwrap();
    });
    world.prepare();
    let (sender, _receiver) = ws_sender();
    world
        .add_client(
            "player-1",
            "Player",
            &sender,
            ClientPreferencesPatch::default(),
            None,
            "bounded-load-attempt".to_owned(),
        )
        .unwrap();

    world.on_request(
        "player-1",
        Message::new(&MessageType::Load)
            .json(
                &serde_json::json!({
                    "center": [999, 999],
                    "direction": [1.0, 0.0],
                    "chunks": [[-1, -2], [5, -2], [6, -2], [-11, -2]]
                })
                .to_string(),
            )
            .build(),
    );

    let entity = world.clients().get("player-1").unwrap().entity;
    let requests = world.read_component::<ChunkRequestsComp>();
    let requests = requests.get(entity).unwrap();
    assert_eq!(requests.center, Vec2(-1, -2));
    assert_eq!(requests.direction, Vec2(0.0, 1.0));
    assert_eq!(requests.requests, vec![Vec2(-1, -2), Vec2(5, -2)]);
}

#[test]
fn legacy_chunk_load_preserves_client_selected_center_and_coordinates() {
    let mut world = test_world("legacy-load", &WorldConfig::default());
    world.prepare();
    let (sender, _receiver) = ws_sender();
    world
        .add_client(
            "player-1",
            "Player",
            &sender,
            ClientPreferencesPatch::default(),
            None,
            "legacy-load-attempt".to_owned(),
        )
        .unwrap();

    world.on_request(
        "player-1",
        Message::new(&MessageType::Load)
            .json(r#"{"center":[999,999],"direction":[1.0,0.0],"chunks":[[999,999]]}"#)
            .build(),
    );

    let entity = world.clients().get("player-1").unwrap().entity;
    let requests = world.read_component::<ChunkRequestsComp>();
    let requests = requests.get(entity).unwrap();
    assert_eq!(requests.center, Vec2(999, 999));
    assert_eq!(requests.requests, vec![Vec2(999, 999)]);
}

#[actix::test]
async fn legacy_transport_init_keeps_global_peer_and_entity_projection() {
    let mut world = test_world("legacy-transport-init", &WorldConfig::default());
    let mut metadata = MetadataComp::new();
    metadata.set_value("loot", serde_json::json!({ "id": "far-loot" }));
    world
        .ecs_mut()
        .create_entity()
        .with(EntityFlag)
        .with(IDComp::new("far-loot"))
        .with(ETypeComp::new("loot", false))
        .with(PositionComp::new(10_000.0, 20.0, 0.0))
        .with(metadata)
        .build();
    world.prepare();

    let (client_sender, _client_receiver) = ws_sender();
    world
        .add_client(
            "legacy-peer",
            "Legacy Peer",
            &client_sender,
            ClientPreferencesPatch::default(),
            None,
            "legacy-peer-attempt".to_owned(),
        )
        .unwrap();

    let (transport_sender, mut transport_receiver) = ws_sender();
    world.add_transport("legacy-transport", &transport_sender);
    let init = crate::decode_message(&transport_receiver.recv().await.unwrap()).unwrap();

    assert!(init.peers.iter().any(|peer| peer.id == "legacy-peer"));
    assert!(init.entities.iter().any(|entity| entity.id == "far-loot"));
}

#[actix::test]
async fn bounded_init_hides_far_peer_and_entity_metadata() {
    let config = WorldConfig::new()
        .entity_visibility_policy(EntityVisibilityPolicy::bounded())
        .entity_visible_radius(96.0)
        .build();
    let mut world = test_world("bounded-init", &config);
    world.set_client_modifier(|world, entity| {
        let id = world.get_id(entity);
        let x = match id.as_str() {
            "near" => 10.0,
            "far" => 200.0,
            _ => 0.0,
        };
        world
            .write_component::<PositionComp>()
            .insert(entity, PositionComp::new(x, 20.0, 0.0))
            .unwrap();
    });
    for (id, x) in [("near-loot", 10.0), ("far-loot", 200.0)] {
        let mut metadata = MetadataComp::new();
        metadata.set_value("loot", serde_json::json!({ "id": id }));
        world
            .ecs_mut()
            .create_entity()
            .with(EntityFlag)
            .with(IDComp::new(id))
            .with(ETypeComp::new("loot", false))
            .with(PositionComp::new(x, 20.0, 0.0))
            .with(metadata)
            .build();
    }
    world.prepare();

    let (viewer_sender, mut viewer_receiver) = ws_sender();
    world
        .add_client(
            "viewer",
            "Viewer",
            &viewer_sender,
            ClientPreferencesPatch::default(),
            None,
            "viewer-attempt".to_owned(),
        )
        .unwrap();
    let viewer_init = crate::decode_message(&viewer_receiver.recv().await.unwrap()).unwrap();
    assert!(viewer_init
        .entities
        .iter()
        .any(|entity| entity.id == "near-loot"));
    assert!(!viewer_init
        .entities
        .iter()
        .any(|entity| entity.id == "far-loot"));

    let (near_sender, mut near_receiver) = ws_sender();
    world
        .add_client(
            "near",
            "Near",
            &near_sender,
            ClientPreferencesPatch::default(),
            None,
            "near-attempt".to_owned(),
        )
        .unwrap();
    let near_init = crate::decode_message(&near_receiver.recv().await.unwrap()).unwrap();
    assert!(near_init.peers.iter().any(|peer| peer.id == "viewer"));

    let (far_sender, mut far_receiver) = ws_sender();
    world
        .add_client(
            "far",
            "Far",
            &far_sender,
            ClientPreferencesPatch::default(),
            None,
            "far-attempt".to_owned(),
        )
        .unwrap();
    let far_init = crate::decode_message(&far_receiver.recv().await.unwrap()).unwrap();
    assert!(!far_init.peers.iter().any(|peer| peer.id == "viewer"));
    assert!(!far_init.peers.iter().any(|peer| peer.id == "near"));
}

#[test]
fn strict_event_allowlist_is_case_insensitive_and_batch_atomic() {
    let blocked_config = WorldConfig::new()
        .request_policy(WorldRequestPolicy::strict().allow_event("pvp:v1:attack"))
        .build();
    let mut blocked_world = test_world("blocked-events", &blocked_config);
    blocked_world.prepare();
    let (blocked_sender, _blocked_receiver) = ws_sender();
    blocked_world
        .add_client(
            "client",
            "Player",
            &blocked_sender,
            ClientPreferencesPatch::default(),
            None,
            "blocked-attempt".to_owned(),
        )
        .unwrap();
    blocked_world.on_request(
        "client",
        Message::new(&MessageType::Event)
            .events(&[
                EventProtocol {
                    name: "PVP:V1:ATTACK".to_owned(),
                    payload: "{}".to_owned(),
                },
                EventProtocol {
                    name: "pvp:v1:forged".to_owned(),
                    payload: "{}".to_owned(),
                },
            ])
            .build(),
    );
    assert!(blocked_world.events().queue.is_empty());

    let allowed_config = WorldConfig::new()
        .request_policy(WorldRequestPolicy::strict().allow_event("pvp:v1:attack"))
        .build();
    let mut allowed_world = test_world("allowed-events", &allowed_config);
    allowed_world.prepare();
    let (allowed_sender, _allowed_receiver) = ws_sender();
    allowed_world
        .add_client(
            "client",
            "Player",
            &allowed_sender,
            ClientPreferencesPatch::default(),
            None,
            "allowed-attempt".to_owned(),
        )
        .unwrap();
    allowed_world.on_request(
        "client",
        Message::new(&MessageType::Event)
            .events(&[EventProtocol {
                name: "PVP:V1:ATTACK".to_owned(),
                payload: "{}".to_owned(),
            }])
            .build(),
    );
    assert_eq!(allowed_world.events().queue.len(), 1);
}

#[test]
fn malformed_public_messages_do_not_panic() {
    let config = WorldConfig::default();
    let mut world = test_world("hardening", &config);
    world.prepare();

    let mut invalid_type = Message::default();
    invalid_type.r#type = i32::MAX;
    world.on_request("client", invalid_type);

    let mut invalid_bulk = Message::new(&MessageType::Update).build();
    invalid_bulk.bulk_update = Some(crate::protocols::BulkUpdate {
        vx: vec![1],
        vy: Vec::new(),
        vz: vec![1],
        voxels: vec![1],
        lights: vec![0],
    });
    world.on_request("client", invalid_bulk);

    assert_eq!(world.lifecycle, WorldLifecycleState::Ready);
}
