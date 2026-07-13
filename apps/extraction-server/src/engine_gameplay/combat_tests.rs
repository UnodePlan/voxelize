use std::time::Duration;

use specs::{Builder, RunNow, WorldExt};
use uuid::Uuid;
use voxelize::{
    Block, Chunk, ChunkOptions, ChunkStatus, DirectionComp, PositionComp, Registry, VoxelAccess,
    World, WorldConfig,
};

use super::{
    authority::GameplayAuthority,
    combat_ordering::drain_attacks_in_stable_order,
    combat_system::CombatResolutionSystem,
    components::{
        CombatComp, EliminationComp, FixedEquipmentComp, HealthComp, MatchPlayerComp, MiningComp,
        ResourceInventoryComp, RoundStatsComp,
    },
    intents::{AttackIntentQueue, QueuedAttackIntent},
    runtime::install_gameplay_runtime,
    tests::match_spec,
    ForcedEliminationQueue,
};
use crate::{
    contracts::{AttackPayload, AttackWeaponSlot, DeathCause, ResourceKey},
    gameplay::{
        combat::{CombatState, HealthState},
        drop_queue::PendingDropQueue,
        inventory::MatchInventory,
        loot::DropId,
        round_stats::RoundStats,
    },
    matchmaking::SeatId,
};

const ATTACKER_POSITION: [f32; 3] = [0.5, 2.5, 0.5];
const VICTIM_POSITION: [f32; 3] = [2.5, 2.5, 0.5];

#[derive(Clone, Copy)]
struct TestPlayer {
    entity: specs::Entity,
    account_id: Uuid,
    public_player_id: Uuid,
    seat_id: SeatId,
}

fn combat_world(now: Duration) -> World {
    let spec = match_spec();
    let config = WorldConfig::new().max_height(64).max_light_level(1).build();
    let mut world = World::new(&spec.world_name, &config);
    install_gameplay_runtime(&mut world, &spec, GameplayAuthority::allow_all_at(now)).unwrap();
    let mut registry = Registry::new();
    registry.register_block(&Block::new("combat-test-solid").id(1).build());
    world.ecs_mut().insert(registry);

    let options = ChunkOptions {
        size: world.config().chunk_size,
        max_height: world.config().max_height,
        sub_chunks: world.config().sub_chunks,
    };
    let mut chunk = Chunk::new("combat-test-chunk", 0, 0, &options);
    chunk.status = ChunkStatus::Ready;
    world.chunks_mut().add(chunk);
    world
}

fn add_occluding_block(world: &mut World) {
    let options = ChunkOptions {
        size: world.config().chunk_size,
        max_height: world.config().max_height,
        sub_chunks: world.config().sub_chunks,
    };
    let mut chunk = Chunk::new("combat-test-occluder", 0, 0, &options);
    chunk.set_raw_voxel(1, 2, 0, 1);
    chunk.status = ChunkStatus::Ready;
    world.chunks_mut().add(chunk);
}

fn add_player(
    world: &mut World,
    roster_index: usize,
    position: [f32; 3],
    direction: [f32; 3],
    health: HealthState,
    inventory: MatchInventory,
) -> TestPlayer {
    let spec = match_spec();
    let participant = spec.roster.iter().nth(roster_index).unwrap();
    let entity = world
        .ecs_mut()
        .create_entity()
        .with(MatchPlayerComp::new(
            participant.account_id,
            participant.public_player_id,
            participant.seat_id,
        ))
        .with(ResourceInventoryComp::new(inventory))
        .with(FixedEquipmentComp::standard())
        .with(MiningComp::new())
        .with(HealthComp::new(health))
        .with(CombatComp::new(CombatState::default()))
        .with(RoundStatsComp::new(RoundStats::new(Duration::ZERO)))
        .with(EliminationComp::alive())
        .with(PositionComp::new(position[0], position[1], position[2]))
        .with(DirectionComp::new(direction[0], direction[1], direction[2]))
        .build();
    TestPlayer {
        entity,
        account_id: participant.account_id,
        public_player_id: participant.public_player_id,
        seat_id: participant.seat_id,
    }
}

fn health_after_hits(hit_count: usize) -> HealthState {
    let mut health = HealthState::new(20).unwrap();
    for _ in 0..hit_count {
        health.apply_damage(2).unwrap();
    }
    health
}

fn inventory_with(resource: ResourceKey, quantity: u32) -> MatchInventory {
    let mut inventory = MatchInventory::new(64).unwrap();
    inventory.insert(resource, quantity).unwrap();
    inventory
}

fn queue_attack(world: &mut World, attacker: TestPlayer, sequence: u32) {
    queue_attack_as(
        world,
        attacker,
        sequence,
        attacker.public_player_id.to_string(),
    );
}

fn queue_attack_as(world: &mut World, attacker: TestPlayer, sequence: u32, client_id: String) {
    world
        .write_resource::<AttackIntentQueue>()
        .push(QueuedAttackIntent {
            entity: attacker.entity,
            client_id,
            request_id: Uuid::new_v4(),
            sequence,
            payload: AttackPayload {
                weapon_slot: AttackWeaponSlot::Melee,
            },
        })
        .unwrap();
}

fn run_combat_at(world: &mut World, now: Duration) {
    world.ecs_mut().insert(GameplayAuthority::allow_all_at(now));
    CombatResolutionSystem.run_now(world.ecs());
    world.ecs_mut().maintain();
}

#[test]
fn mismatched_attacker_identity_cannot_damage_a_target() {
    let now = Duration::from_secs(5);
    let mut world = combat_world(now);
    let attacker = add_player(
        &mut world,
        0,
        ATTACKER_POSITION,
        [1.0, 0.0, 0.0],
        HealthState::new(20).unwrap(),
        MatchInventory::new(64).unwrap(),
    );
    let victim = add_player(
        &mut world,
        1,
        VICTIM_POSITION,
        [-1.0, 0.0, 0.0],
        HealthState::new(20).unwrap(),
        MatchInventory::new(64).unwrap(),
    );

    queue_attack_as(&mut world, attacker, 1, "detached-client".to_owned());
    run_combat_at(&mut world, now);

    assert_eq!(
        world
            .read_component::<HealthComp>()
            .get(victim.entity)
            .unwrap()
            .state()
            .half_hearts(),
        20
    );
    assert_eq!(
        world
            .read_component::<CombatComp>()
            .get(attacker.entity)
            .unwrap()
            .state()
            .last_sequence(),
        None,
        "未绑定身份必须在消费攻击序号前被拒绝"
    );
}

#[test]
fn same_player_attack_order_preserves_network_arrival() {
    let now = Duration::from_secs(5);
    let mut world = combat_world(now);
    let attacker = add_player(
        &mut world,
        0,
        ATTACKER_POSITION,
        [1.0, 0.0, 0.0],
        HealthState::new(20).unwrap(),
        MatchInventory::new(64).unwrap(),
    );
    queue_attack(&mut world, attacker, 2);
    queue_attack(&mut world, attacker, 1);

    let mut intents = world.ecs().write_resource::<AttackIntentQueue>();
    let players = world.ecs().read_storage::<MatchPlayerComp>();
    let sequences = drain_attacks_in_stable_order(&mut intents, &players)
        .into_iter()
        .map(|intent| intent.sequence)
        .collect::<Vec<_>>();

    assert_eq!(sequences, vec![2, 1]);
}

#[test]
fn lethal_melee_commits_exactly_one_terminal_drop_even_after_later_attack() {
    let now = Duration::from_secs(10);
    let mut world = combat_world(now);
    let attacker = add_player(
        &mut world,
        0,
        ATTACKER_POSITION,
        [1.0, 0.0, 0.0],
        HealthState::new(20).unwrap(),
        MatchInventory::new(64).unwrap(),
    );
    let victim = add_player(
        &mut world,
        1,
        VICTIM_POSITION,
        [-1.0, 0.0, 0.0],
        health_after_hits(9),
        inventory_with(ResourceKey::Diamond, 17),
    );

    queue_attack(&mut world, attacker, 1);
    run_combat_at(&mut world, now);

    assert_eq!(
        world
            .read_component::<HealthComp>()
            .get(victim.entity)
            .unwrap()
            .state()
            .half_hearts(),
        0
    );
    assert_eq!(
        world
            .read_component::<ResourceInventoryComp>()
            .get(victim.entity)
            .unwrap()
            .inventory()
            .total_quantity(),
        0
    );
    assert_eq!(
        world.read_resource::<PendingDropQueue>().total_quantity(),
        17
    );

    // 冷却结束后的新挥击也不能再次命中已经进入终态的目标。
    queue_attack(&mut world, attacker, 2);
    run_combat_at(&mut world, now + Duration::from_millis(600));

    let elimination = world.read_component::<EliminationComp>();
    let record = elimination.get(victim.entity).unwrap().record().unwrap();
    assert_eq!(record.result.data.cause, DeathCause::Melee);
    assert_eq!(record.killer_account_id, Some(attacker.account_id));
    drop(elimination);
    assert_eq!(
        world.read_resource::<PendingDropQueue>().total_quantity(),
        17
    );
    assert_eq!(
        world
            .read_component::<RoundStatsComp>()
            .get(attacker.entity)
            .unwrap()
            .stats()
            .kills(),
        1
    );

    let drops = world.write_resource::<PendingDropQueue>().drain_sorted();
    assert_eq!(drops.len(), 1);
    assert_eq!(
        drops[0].id(),
        &DropId::death(match_spec().match_id, victim.seat_id.get())
    );
    assert_eq!(drops[0].contents().quantity(ResourceKey::Diamond), 17);
}

#[test]
fn ten_authoritative_melee_hits_kill_only_on_the_tenth() {
    let start = Duration::from_secs(10);
    let mut world = combat_world(start);
    let attacker = add_player(
        &mut world,
        0,
        ATTACKER_POSITION,
        [1.0, 0.0, 0.0],
        HealthState::new(20).unwrap(),
        MatchInventory::new(64).unwrap(),
    );
    let victim = add_player(
        &mut world,
        1,
        VICTIM_POSITION,
        [-1.0, 0.0, 0.0],
        HealthState::new(20).unwrap(),
        inventory_with(ResourceKey::Dirt, 10),
    );

    for sequence in 1..=10 {
        queue_attack(&mut world, attacker, sequence);
        run_combat_at(
            &mut world,
            start + Duration::from_millis(u64::from(sequence - 1) * 600),
        );
        let remaining = world
            .read_component::<HealthComp>()
            .get(victim.entity)
            .unwrap()
            .state()
            .half_hearts();
        assert_eq!(remaining, 20 - sequence as u8 * 2);
        assert_eq!(
            world
                .read_component::<EliminationComp>()
                .get(victim.entity)
                .unwrap()
                .record()
                .is_some(),
            sequence == 10
        );
    }

    assert_eq!(
        world.read_resource::<PendingDropQueue>().total_quantity(),
        10
    );
    assert_eq!(
        world
            .read_component::<RoundStatsComp>()
            .get(attacker.entity)
            .unwrap()
            .stats()
            .kills(),
        1
    );
}

#[test]
fn a_miss_consumes_cooldown_before_a_later_hit() {
    let start = Duration::from_secs(5);
    let mut world = combat_world(start);
    let attacker = add_player(
        &mut world,
        0,
        ATTACKER_POSITION,
        [-1.0, 0.0, 0.0],
        HealthState::new(20).unwrap(),
        MatchInventory::new(64).unwrap(),
    );
    let victim = add_player(
        &mut world,
        1,
        VICTIM_POSITION,
        [-1.0, 0.0, 0.0],
        HealthState::new(20).unwrap(),
        MatchInventory::new(64).unwrap(),
    );

    queue_attack(&mut world, attacker, 1);
    run_combat_at(&mut world, start);
    world
        .write_component::<DirectionComp>()
        .get_mut(attacker.entity)
        .unwrap()
        .0 = voxelize::Vec3(1.0, 0.0, 0.0);
    queue_attack(&mut world, attacker, 2);
    run_combat_at(&mut world, start + Duration::from_millis(599));
    assert_eq!(
        world
            .read_component::<HealthComp>()
            .get(victim.entity)
            .unwrap()
            .state()
            .half_hearts(),
        20
    );

    queue_attack(&mut world, attacker, 3);
    run_combat_at(&mut world, start + Duration::from_millis(600));
    assert_eq!(
        world
            .read_component::<HealthComp>()
            .get(victim.entity)
            .unwrap()
            .state()
            .half_hearts(),
        18
    );
}

#[test]
fn detached_target_can_be_hit_but_dead_target_is_permanently_excluded() {
    let now = Duration::from_secs(5);
    let mut world = combat_world(now);
    let attacker = add_player(
        &mut world,
        0,
        ATTACKER_POSITION,
        [1.0, 0.0, 0.0],
        HealthState::new(20).unwrap(),
        MatchInventory::new(64).unwrap(),
    );
    let victim = add_player(
        &mut world,
        1,
        VICTIM_POSITION,
        [-1.0, 0.0, 0.0],
        HealthState::new(20).unwrap(),
        MatchInventory::new(64).unwrap(),
    );
    assert!(world
        .clients()
        .get(&victim.public_player_id.to_string())
        .is_none());

    queue_attack(&mut world, attacker, 1);
    run_combat_at(&mut world, now);
    assert_eq!(
        world
            .read_component::<HealthComp>()
            .get(victim.entity)
            .unwrap()
            .state()
            .half_hearts(),
        18,
        "断线只冻结主动操作，角色仍应留场承伤"
    );

    world
        .read_resource::<ForcedEliminationQueue>()
        .enqueue(victim.account_id)
        .unwrap();
    run_combat_at(&mut world, now + Duration::from_millis(600));
    let dead_revision = world
        .read_component::<HealthComp>()
        .get(victim.entity)
        .unwrap()
        .state()
        .revision();

    queue_attack(&mut world, attacker, 2);
    run_combat_at(&mut world, now + Duration::from_millis(1_200));
    let health = world.read_component::<HealthComp>();
    let state = health.get(victim.entity).unwrap().state();
    assert_eq!(state.half_hearts(), 0);
    assert_eq!(state.revision(), dead_revision);
}

#[test]
fn melee_selects_the_nearest_in_range_target() {
    let now = Duration::from_secs(5);
    let mut world = combat_world(now);
    let attacker = add_player(
        &mut world,
        0,
        ATTACKER_POSITION,
        [1.0, 0.0, 0.0],
        HealthState::new(20).unwrap(),
        MatchInventory::new(64).unwrap(),
    );
    let nearest = add_player(
        &mut world,
        1,
        [1.8, 2.5, 0.5],
        [-1.0, 0.0, 0.0],
        HealthState::new(20).unwrap(),
        MatchInventory::new(64).unwrap(),
    );
    let farther = add_player(
        &mut world,
        2,
        [2.8, 2.5, 0.5],
        [-1.0, 0.0, 0.0],
        HealthState::new(20).unwrap(),
        MatchInventory::new(64).unwrap(),
    );

    queue_attack(&mut world, attacker, 1);
    run_combat_at(&mut world, now);

    let health = world.read_component::<HealthComp>();
    assert_eq!(
        health.get(nearest.entity).unwrap().state().half_hearts(),
        18
    );
    assert_eq!(
        health.get(farther.entity).unwrap().state().half_hearts(),
        20
    );
}

#[test]
fn melee_reach_accepts_three_blocks_and_rejects_just_beyond() {
    let now = Duration::from_secs(5);
    for (victim_x, expected_health) in [(3.9, 18), (3.901, 20)] {
        let mut world = combat_world(now);
        let attacker = add_player(
            &mut world,
            0,
            ATTACKER_POSITION,
            [1.0, 0.0, 0.0],
            HealthState::new(20).unwrap(),
            MatchInventory::new(64).unwrap(),
        );
        let victim = add_player(
            &mut world,
            1,
            [victim_x, 2.5, 0.5],
            [-1.0, 0.0, 0.0],
            HealthState::new(20).unwrap(),
            MatchInventory::new(64).unwrap(),
        );

        queue_attack(&mut world, attacker, 1);
        run_combat_at(&mut world, now);

        assert_eq!(
            world
                .read_component::<HealthComp>()
                .get(victim.entity)
                .unwrap()
                .state()
                .half_hearts(),
            expected_health
        );
    }
}

#[test]
fn full_block_occlusion_prevents_melee_damage() {
    let now = Duration::from_secs(5);
    let mut world = combat_world(now);
    add_occluding_block(&mut world);
    let attacker = add_player(
        &mut world,
        0,
        ATTACKER_POSITION,
        [1.0, 0.0, 0.0],
        HealthState::new(20).unwrap(),
        MatchInventory::new(64).unwrap(),
    );
    let victim = add_player(
        &mut world,
        1,
        VICTIM_POSITION,
        [-1.0, 0.0, 0.0],
        HealthState::new(20).unwrap(),
        MatchInventory::new(64).unwrap(),
    );

    queue_attack(&mut world, attacker, 1);
    run_combat_at(&mut world, now);

    assert_eq!(
        world
            .read_component::<HealthComp>()
            .get(victim.entity)
            .unwrap()
            .state()
            .half_hearts(),
        20
    );
}

#[test]
fn timeout_wins_same_tick_attack_race_and_produces_one_terminal_drop() {
    let now = Duration::from_secs(60);
    let mut world = combat_world(now);
    let attacker = add_player(
        &mut world,
        0,
        ATTACKER_POSITION,
        [1.0, 0.0, 0.0],
        HealthState::new(20).unwrap(),
        MatchInventory::new(64).unwrap(),
    );
    let victim = add_player(
        &mut world,
        1,
        VICTIM_POSITION,
        [-1.0, 0.0, 0.0],
        health_after_hits(9),
        inventory_with(ResourceKey::Gold, 23),
    );

    world
        .read_resource::<ForcedEliminationQueue>()
        .enqueue(victim.account_id)
        .unwrap();
    queue_attack(&mut world, attacker, 1);
    run_combat_at(&mut world, now);

    let eliminations = world.read_component::<EliminationComp>();
    let record = eliminations.get(victim.entity).unwrap().record().unwrap();
    assert_eq!(record.result.data.cause, DeathCause::ReconnectTimeout);
    assert_eq!(record.killer_account_id, None);
    drop(eliminations);
    assert_eq!(
        world
            .read_component::<RoundStatsComp>()
            .get(attacker.entity)
            .unwrap()
            .stats()
            .kills(),
        0
    );
    assert_eq!(
        world.read_resource::<PendingDropQueue>().total_quantity(),
        23
    );

    // 重复超时与后续攻击都必须观察到同一个终态，不能复制掉落。
    world
        .read_resource::<ForcedEliminationQueue>()
        .enqueue(victim.account_id)
        .unwrap();
    queue_attack(&mut world, attacker, 2);
    run_combat_at(&mut world, now + Duration::from_millis(600));
    assert_eq!(
        world.read_resource::<PendingDropQueue>().total_quantity(),
        23
    );
    assert_eq!(
        world
            .write_resource::<PendingDropQueue>()
            .drain_sorted()
            .len(),
        1
    );
}
