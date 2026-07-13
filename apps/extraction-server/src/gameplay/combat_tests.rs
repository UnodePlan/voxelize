use std::time::Duration;

use super::{
    combat::{
        CombatError, CombatState, DamageOutcome, HealthError, HealthState, SwingOutcome,
        BASIC_MELEE_COOLDOWN, BASIC_MELEE_DAMAGE_HALF_HEARTS, BASIC_MELEE_REACH,
    },
    loot::ResourceBundle,
    ray_aabb::{ray_aabb_distance, RayAabbError, RayBounds},
    round_stats::{RoundStats, RoundStatsError},
};
use crate::{contracts::ResourceKey, match_world::MAX_HEALTH_HALF_HEARTS};

#[test]
fn minecraft_style_damage_kills_on_the_tenth_hit() {
    let mut health = HealthState::new(MAX_HEALTH_HALF_HEARTS).unwrap();
    assert_eq!(health.max_half_hearts(), 20);

    for remaining in (1..10).rev().map(|hearts| hearts * 2) {
        assert_eq!(
            health.apply_damage(BASIC_MELEE_DAMAGE_HALF_HEARTS),
            Ok(DamageOutcome::Damaged {
                remaining_half_hearts: remaining,
            })
        );
    }
    assert_eq!(
        health.apply_damage(BASIC_MELEE_DAMAGE_HALF_HEARTS),
        Ok(DamageOutcome::Killed)
    );
    assert_eq!(health.half_hearts(), 0);
    assert_eq!(health.revision(), 10);
    assert!(!health.is_alive());

    let terminal = health.clone();
    assert_eq!(health.eliminate(), Err(HealthError::AlreadyDead));
    assert_eq!(health, terminal);
}

#[test]
fn health_rejects_invalid_values_and_revision_overflow_atomically() {
    assert_eq!(HealthState::new(0), Err(HealthError::InvalidMaximum));
    let mut health = HealthState::new(20).unwrap();
    assert_eq!(health.apply_damage(0), Err(HealthError::InvalidDamage));
    health.set_revision_for_test(u32::MAX);
    let before = health.clone();
    assert_eq!(health.apply_damage(2), Err(HealthError::RevisionExhausted));
    assert_eq!(health, before);
}

#[test]
fn swing_sequence_and_cooldown_have_inclusive_boundary() {
    assert_eq!(BASIC_MELEE_REACH, 3.0);
    let mut combat = CombatState::default();
    assert_eq!(
        combat.accept_swing(1, Duration::ZERO, BASIC_MELEE_COOLDOWN),
        Ok(SwingOutcome::Accepted)
    );
    assert_eq!(combat.last_attack_at(), Some(Duration::ZERO));

    assert_eq!(
        combat.accept_swing(2, Duration::from_millis(599), BASIC_MELEE_COOLDOWN),
        Ok(SwingOutcome::Cooldown {
            ready_at: Duration::from_millis(600),
        })
    );
    assert_eq!(combat.last_sequence(), Some(2));
    assert_eq!(combat.last_attack_at(), Some(Duration::ZERO));
    assert_eq!(
        combat.accept_swing(2, Duration::from_millis(600), BASIC_MELEE_COOLDOWN),
        Err(CombatError::StaleSequence)
    );
    assert_eq!(
        combat.accept_swing(3, Duration::from_millis(600), BASIC_MELEE_COOLDOWN),
        Ok(SwingOutcome::Accepted)
    );
    assert_eq!(combat.revision(), 3);
}

#[test]
fn combat_failures_do_not_partially_mutate_state() {
    let mut combat = CombatState::default();
    assert_eq!(
        combat.accept_swing(1, Duration::from_secs(1), BASIC_MELEE_COOLDOWN),
        Ok(SwingOutcome::Accepted)
    );

    let before_regression = combat.clone();
    assert_eq!(
        combat.accept_swing(2, Duration::ZERO, BASIC_MELEE_COOLDOWN),
        Err(CombatError::TimeRegression)
    );
    assert_eq!(combat, before_regression);

    combat.set_revision_for_test(u32::MAX);
    let before_overflow = combat.clone();
    assert_eq!(
        combat.reject_sequence(2),
        Err(CombatError::RevisionExhausted)
    );
    assert_eq!(combat, before_overflow);
}

#[test]
fn ray_aabb_returns_world_distance_for_non_unit_direction() {
    let bounds = RayBounds {
        min: [-0.5, 0.0, -0.5],
        max: [0.5, 1.8, 0.5],
    };
    assert_eq!(
        ray_aabb_distance([0.0, 0.9, -2.0], [0.0, 0.0, 2.0], bounds, 1.5),
        Ok(Some(1.5))
    );
    assert_eq!(
        ray_aabb_distance([0.0, 0.9, -2.0], [0.0, 0.0, 2.0], bounds, 1.49),
        Ok(None)
    );
    assert_eq!(
        ray_aabb_distance([0.0, 0.9, 0.0], [1.0, 0.0, 0.0], bounds, 0.0),
        Ok(Some(0.0))
    );
}

#[test]
fn ray_aabb_handles_parallel_miss_behind_and_invalid_inputs() {
    let bounds = RayBounds {
        min: [0.0, 0.0, 0.0],
        max: [1.0, 1.0, 1.0],
    };
    assert_eq!(
        ray_aabb_distance([2.0, 0.5, -1.0], [0.0, 0.0, 1.0], bounds, 10.0),
        Ok(None)
    );
    assert_eq!(
        ray_aabb_distance([0.5, 0.5, 2.0], [0.0, 0.0, 1.0], bounds, 10.0),
        Ok(None)
    );
    assert_eq!(
        ray_aabb_distance([0.0; 3], [0.0; 3], bounds, 1.0),
        Err(RayAabbError::Direction)
    );
    assert_eq!(
        ray_aabb_distance([f32::NAN, 0.0, 0.0], [1.0, 0.0, 0.0], bounds, 1.0),
        Err(RayAabbError::Origin)
    );
    assert_eq!(
        ray_aabb_distance(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            RayBounds {
                min: [1.0, 0.0, 0.0],
                max: [0.0, 1.0, 1.0],
            },
            1.0,
        ),
        Err(RayAabbError::Bounds)
    );
    assert_eq!(
        ray_aabb_distance([0.0; 3], [1.0, 0.0, 0.0], bounds, -1.0),
        Err(RayAabbError::MaxDistance)
    );
}

#[test]
fn round_stats_accumulate_each_bucket_and_finish_once() {
    let mut stats = RoundStats::new(Duration::from_secs(5));
    assert!(stats.record_mined(ResourceKey::Gold, 3).unwrap());
    assert!(stats.record_mined(ResourceKey::Gold, 2).unwrap());
    assert!(stats
        .record_picked_up(ResourceBundle::new(1, 2, 3))
        .unwrap());
    assert!(stats.record_lost(ResourceBundle::new(0, 4, 1)).unwrap());
    stats.record_kill().unwrap();
    assert!(!stats.record_mined(ResourceKey::Dirt, 0).unwrap());
    assert!(!stats.record_lost(ResourceBundle::default()).unwrap());

    assert_eq!(stats.mined().quantity(ResourceKey::Gold), 5);
    assert_eq!(stats.picked_up(), ResourceBundle::new(1, 2, 3));
    assert_eq!(stats.lost(), ResourceBundle::new(0, 4, 1));
    assert_eq!(stats.kills(), 1);
    assert_eq!(stats.survival_started_at(), Duration::from_secs(5));
    assert_eq!(stats.revision(), 5);
    assert!(stats.finish_survival(Duration::from_secs(9)).unwrap());
    assert_eq!(stats.survival_ended_at(), Some(Duration::from_secs(9)));
    assert!(!stats.finish_survival(Duration::from_secs(10)).unwrap());
    assert_eq!(stats.revision(), 6);
}

#[test]
fn round_stats_overflow_and_time_errors_are_atomic() {
    let mut stats = RoundStats::new(Duration::from_secs(5));
    stats
        .record_picked_up(ResourceBundle::new(u32::MAX, 0, 0))
        .unwrap();
    let before_quantity_overflow = stats.clone();
    assert_eq!(
        stats.record_picked_up(ResourceBundle::new(1, 0, 0)),
        Err(RoundStatsError::QuantityOverflow)
    );
    assert_eq!(stats, before_quantity_overflow);
    assert_eq!(
        stats.finish_survival(Duration::from_secs(4)),
        Err(RoundStatsError::TimeRegression)
    );
    assert_eq!(stats, before_quantity_overflow);

    stats.set_revision_for_test(u32::MAX);
    let before_revision_overflow = stats.clone();
    assert_eq!(stats.record_kill(), Err(RoundStatsError::RevisionExhausted));
    assert_eq!(stats, before_revision_overflow);
}
