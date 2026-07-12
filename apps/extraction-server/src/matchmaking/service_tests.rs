use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration as StdDuration, SystemTime},
};

use async_trait::async_trait;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::coordinator_lifecycle::stop_world_once;
use super::*;
use crate::{
    match_world::{FixedMatchLoadout, PlayableBounds, ENGINE_MAX_CHUNK, ENGINE_MIN_CHUNK},
    ports::{
        Clock, IdGenerator, MatchRepository, MatchRepositoryError, MatchWorldRuntime,
        MatchWorldRuntimeError, MatchWorldSpec, PreparedMatchWorld, RepositoryFuture,
        RepositoryProbe, SeedGenerator, SettlingTrigger, TransitionOutcome,
    },
};

const TEST_WORLD_GENERATION: &str = "test-world-generation";

#[tokio::test]
async fn queue_fails_closed_until_world_runtime_is_bound() {
    let account_id = accounts(1)[0];
    let service = MatchmakingService::start(
        Arc::new(MemoryMatchRepository::default()),
        Arc::new(ManualClock::default()),
        Arc::new(SequenceIds::default()),
        Arc::new(FixedSeed),
        MatchVersions {
            generation: "generation-v1".to_owned(),
            gameplay: "gameplay-v1".to_owned(),
            config: "config-v1".to_owned(),
        },
    );
    service
        .apply_connection_event(MatchConnectionEvent::Connected {
            connection_id: connection_id(0),
            account_id,
        })
        .await
        .unwrap();

    assert_eq!(
        service.enqueue(account_id).await,
        Err(MatchmakingError::Unavailable)
    );
}

#[tokio::test]
async fn world_stop_call_has_a_finite_timeout() {
    let runtime = MemoryWorldRuntime::default();
    runtime.hang_stop.store(true, Ordering::SeqCst);

    assert_eq!(
        stop_world_once(
            &runtime,
            Uuid::from_u128(1),
            "hanging-world",
            StdDuration::from_millis(1),
        )
        .await,
        Err(MatchmakingError::Unavailable)
    );
}

#[tokio::test]
async fn stale_world_disconnect_removes_last_connection_from_waiting_queue() {
    let harness = Harness::new().await;
    let account_id = accounts(1)[0];
    harness.connect_all(&[account_id]).await;
    assert_eq!(
        harness.service.enqueue(account_id).await.unwrap().status,
        QueueStatus::Queued
    );

    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Disconnected {
            connection_id: connection_id(0),
            account_id,
            observed_at: harness.clock.monotonic_now(),
            world_name: Some("old-match".to_owned()),
            world_generation: Some("old-generation".to_owned()),
            client_id: Some("old-client".to_owned()),
            attach_attempt_id: Some("old-attempt".to_owned()),
        })
        .await
        .unwrap();

    assert_eq!(
        harness.service.enqueue(account_id).await,
        Err(MatchmakingError::ConnectionRequired)
    );
}

#[tokio::test]
async fn exact_ten_freeze_and_eleventh_rejection_are_serialized() {
    let harness = Harness::new().await;
    let accounts = accounts(11);
    harness.connect_all(&accounts).await;

    for (index, account_id) in accounts.iter().take(9).enumerate() {
        let snapshot = harness.service.enqueue(*account_id).await.unwrap();
        assert_eq!(snapshot.status, QueueStatus::Queued);
        assert_eq!(snapshot.position, Some(index + 1));
    }
    let preparing = harness.service.enqueue(accounts[9]).await.unwrap();
    assert_eq!(preparing.status, QueueStatus::Preparing);
    assert_eq!(
        harness.service.enqueue(accounts[10]).await,
        Err(MatchmakingError::Full)
    );
    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Disconnected {
            connection_id: connection_id(10),
            account_id: accounts[10],
            observed_at: harness.clock.monotonic_now(),
            world_name: None,
            world_generation: None,
            client_id: None,
            attach_attempt_id: None,
        })
        .await
        .unwrap();

    let spec = harness.runtime.only_spec();
    assert_eq!(spec.roster.iter().len(), 10);
    assert_eq!(spec.engine_min_chunk, ENGINE_MIN_CHUNK);
    assert_eq!(spec.engine_max_chunk, ENGINE_MAX_CHUNK);
    assert_eq!(spec.playable_bounds, PlayableBounds::EXTRACTION);
    assert_eq!(spec.loadout, FixedMatchLoadout::default());
    assert!(!spec.saving);
    assert!(!harness.service.allows_attach(
        &spec.world_name,
        TEST_WORLD_GENERATION,
        "outsider-client",
        "outsider-attempt",
        accounts[10],
        MatchAttachKind::Join,
    ));

    harness.join_all(&accounts[..10], &spec.world_name).await;
    let active = harness.service.enqueue(accounts[0]).await.unwrap();
    assert_eq!(active.status, QueueStatus::Active);
    assert!(!harness.service.allows_attach(
        &spec.world_name,
        TEST_WORLD_GENERATION,
        "outsider-client",
        "outsider-attempt",
        accounts[10],
        MatchAttachKind::Join,
    ));

    let stored = harness.repository.only_match();
    assert_eq!(stored.record.state, MatchState::Active);
    assert_eq!(stored.participants.len(), 10);
    assert_eq!(
        stored.record.extraction_open_at.unwrap() - stored.record.started_at.unwrap(),
        Duration::minutes(8)
    );
    assert_eq!(
        stored.record.hard_deadline.unwrap() - stored.record.started_at.unwrap(),
        Duration::minutes(12)
    );
}

#[tokio::test]
async fn eleven_concurrent_requests_admit_exactly_ten_accounts() {
    let harness = Harness::new().await;
    let accounts = accounts(11);
    harness.connect_all(&accounts).await;
    let mut requests = Vec::new();
    for account_id in accounts {
        let service = harness.service.clone();
        requests.push(tokio::spawn(
            async move { service.enqueue(account_id).await },
        ));
    }

    let mut admitted = 0;
    let mut full = 0;
    for request in requests {
        match request.await.unwrap() {
            Ok(_) => admitted += 1,
            Err(MatchmakingError::Full) => full += 1,
            result => panic!("并发排队返回了意外结果: {result:?}"),
        }
    }
    assert_eq!(admitted, 10);
    assert_eq!(full, 1);
    assert_eq!(harness.runtime.only_spec().roster.iter().len(), 10);
}

#[tokio::test]
async fn uncertain_create_retries_the_same_prepare_attempt() {
    let harness = Harness::new().await;
    let accounts = accounts(10);
    harness.connect_all(&accounts).await;
    for account_id in accounts.iter().take(9) {
        harness.service.enqueue(*account_id).await.unwrap();
    }
    harness.repository.fail_next_create_after_store();
    assert_eq!(
        harness.service.enqueue(accounts[9]).await,
        Err(MatchmakingError::Unavailable)
    );
    let persisted_match_id = harness.repository.only_match().record.match_id;

    let preparing = harness.service.enqueue(accounts[9]).await.unwrap();
    assert_eq!(preparing.match_id, Some(persisted_match_id));
    assert_eq!(harness.repository.match_count(), 1);
}

#[tokio::test]
async fn uncertain_create_is_aborted_before_disconnected_player_leaves_queue() {
    let harness = Harness::new().await;
    let accounts = accounts(10);
    harness.connect_all(&accounts).await;
    for account_id in accounts.iter().take(9) {
        harness.service.enqueue(*account_id).await.unwrap();
    }
    harness.repository.fail_next_create_after_store();
    assert_eq!(
        harness.service.enqueue(accounts[9]).await,
        Err(MatchmakingError::Unavailable)
    );
    let abandoned_match_id = harness.repository.only_match().record.match_id;

    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Disconnected {
            connection_id: connection_id(9),
            account_id: accounts[9],
            observed_at: harness.clock.monotonic_now(),
            world_name: None,
            world_generation: None,
            client_id: None,
            attach_attempt_id: None,
        })
        .await
        .unwrap();
    assert_eq!(
        harness.repository.only_match().record.state,
        MatchState::Aborted
    );
    assert_eq!(harness.runtime.spec_count(), 0);

    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Connected {
            connection_id: "connection-9-reconnected".to_owned(),
            account_id: accounts[9],
        })
        .await
        .unwrap();
    let replacement = harness.service.enqueue(accounts[9]).await.unwrap();
    assert_eq!(replacement.status, QueueStatus::Preparing);
    assert_ne!(replacement.match_id, Some(abandoned_match_id));
    assert_eq!(harness.repository.match_count(), 2);
}

#[tokio::test]
async fn preparing_disconnect_aborts_and_restores_original_fifo() {
    let harness = Harness::new().await;
    let accounts = accounts(10);
    harness.connect_all(&accounts).await;
    let mut enqueued_at = HashMap::new();
    for account_id in accounts.iter().take(9) {
        let snapshot = harness.service.enqueue(*account_id).await.unwrap();
        enqueued_at.insert(*account_id, snapshot.enqueued_at.unwrap());
        harness.clock.advance(StdDuration::from_secs(1));
    }
    let preparing = harness.service.enqueue(accounts[9]).await.unwrap();
    let aborted_match_id = preparing.match_id.unwrap();
    let world_name = preparing.world_name.unwrap();
    let spec = harness.runtime.only_spec();
    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Connected {
            connection_id: "connection-4-backup".to_owned(),
            account_id: accounts[4],
        })
        .await
        .unwrap();

    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Disconnected {
            connection_id: connection_id(4),
            account_id: accounts[4],
            observed_at: harness.clock.monotonic_now(),
            world_name: Some(world_name.clone()),
            world_generation: Some(TEST_WORLD_GENERATION.to_owned()),
            client_id: Some(public_player_id(&spec, accounts[4])),
            attach_attempt_id: Some("join-attempt-4".to_owned()),
        })
        .await
        .unwrap();

    assert_eq!(harness.runtime.stop_count(), 1);
    assert_eq!(
        harness.repository.only_match().record.state,
        MatchState::Aborted
    );
    assert!(!harness.service.allows_attach(
        &world_name,
        TEST_WORLD_GENERATION,
        &public_player_id(&spec, accounts[0]),
        "join-attempt-0",
        accounts[0],
        MatchAttachKind::Join,
    ));
    let next = harness.service.enqueue(accounts[4]).await.unwrap();
    let next_match_id = next.match_id.unwrap();
    assert_eq!(next.status, QueueStatus::Preparing);
    assert_ne!(next_match_id, aborted_match_id);
    assert_eq!(harness.repository.match_count(), 2);
    assert_eq!(harness.runtime.spec_count(), 2);

    let next_match = harness.repository.match_by_id(next_match_id);
    for account_id in accounts.iter().take(9) {
        let participant = next_match
            .participants
            .iter()
            .find(|participant| participant.account_id == *account_id)
            .unwrap();
        assert_eq!(participant.enqueued_at, enqueued_at[account_id]);
    }
}

#[tokio::test]
async fn preparing_lobby_socket_disconnect_does_not_abort_match_socket() {
    let harness = Harness::new().await;
    let accounts = accounts(10);
    harness.connect_all(&accounts).await;
    for account_id in &accounts {
        harness.service.enqueue(*account_id).await.unwrap();
    }
    let spec = harness.runtime.only_spec();
    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Connected {
            connection_id: "account-0-lobby".to_owned(),
            account_id: accounts[0],
        })
        .await
        .unwrap();

    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Disconnected {
            connection_id: "account-0-lobby".to_owned(),
            account_id: accounts[0],
            observed_at: harness.clock.monotonic_now(),
            world_name: None,
            world_generation: None,
            client_id: None,
            attach_attempt_id: None,
        })
        .await
        .unwrap();

    assert_eq!(harness.runtime.stop_count(), 0);
    assert_eq!(
        harness.repository.only_match().record.state,
        MatchState::Preparing
    );
    assert!(harness.service.allows_attach(
        &spec.world_name,
        TEST_WORLD_GENERATION,
        &public_player_id(&spec, accounts[0]),
        "join-attempt-0",
        accounts[0],
        MatchAttachKind::Join,
    ));
}

#[tokio::test]
async fn settling_persistence_failure_keeps_world_closed_and_retries_once() {
    let harness = Harness::new().await;
    let accounts = accounts(10);
    harness.connect_all(&accounts).await;
    for account_id in &accounts {
        harness.service.enqueue(*account_id).await.unwrap();
    }
    let world_name = harness.runtime.only_spec().world_name;
    harness.join_all(&accounts, &world_name).await;

    harness.clock.advance(StdDuration::from_secs(12 * 60));
    harness.repository.fail_next_begin_settling();
    assert_eq!(
        harness.service.tick().await,
        Err(MatchmakingError::Unavailable)
    );
    assert_eq!(harness.runtime.stop_count(), 1);
    assert_eq!(
        harness.service.enqueue(accounts[0]).await.unwrap().status,
        QueueStatus::Settling
    );

    harness.service.tick().await.unwrap();
    assert_eq!(harness.runtime.stop_count(), 1);
    assert_eq!(
        harness.repository.only_match().record.state,
        MatchState::Finished
    );
}

#[tokio::test]
async fn repository_latency_does_not_extend_absolute_match_deadlines() {
    let harness = Harness::new().await;
    let accounts = accounts(10);
    harness.connect_all(&accounts).await;
    for account_id in &accounts {
        harness.service.enqueue(*account_id).await.unwrap();
    }
    let clock = harness.clock.clone();
    harness
        .repository
        .on_activate(move || clock.advance(StdDuration::from_secs(30)));
    let spec = harness.runtime.only_spec();
    harness.join_all(&accounts, &spec.world_name).await;

    harness.clock.set(StdDuration::from_secs(8 * 60));
    harness.service.tick().await.unwrap();
    assert_eq!(
        harness.service.enqueue(accounts[0]).await.unwrap().status,
        QueueStatus::ExtractionOpen
    );
    harness.clock.set(StdDuration::from_secs(12 * 60));
    harness.service.tick().await.unwrap();
    assert_eq!(
        harness.repository.only_match().record.state,
        MatchState::Finished
    );
}

#[tokio::test]
async fn activation_returning_after_hard_deadline_never_opens_gameplay() {
    let harness = Harness::new().await;
    let accounts = accounts(10);
    harness.connect_all(&accounts).await;
    for account_id in &accounts {
        harness.service.enqueue(*account_id).await.unwrap();
    }
    let clock = harness.clock.clone();
    harness
        .repository
        .on_activate(move || clock.advance(StdDuration::from_secs(13 * 60)));
    let spec = harness.runtime.only_spec();
    harness.join_all(&accounts, &spec.world_name).await;

    assert_eq!(harness.runtime.stop_count(), 1);
    assert_eq!(
        harness.repository.only_match().record.state,
        MatchState::Settling
    );
    assert_eq!(
        harness.service.enqueue(accounts[0]).await.unwrap().status,
        QueueStatus::Settling
    );
    assert!(!harness.service.allows_attach(
        &spec.world_name,
        TEST_WORLD_GENERATION,
        &public_player_id(&spec, accounts[0]),
        "late-join-attempt",
        accounts[0],
        MatchAttachKind::Join,
    ));

    harness.service.tick().await.unwrap();
    assert_eq!(
        harness.repository.only_match().record.state,
        MatchState::Finished
    );
}

#[tokio::test]
async fn disconnect_observed_while_activation_is_in_flight_aborts_match() {
    let harness = Harness::new().await;
    let accounts = accounts(10);
    harness.connect_all(&accounts).await;
    for account_id in &accounts {
        harness.service.enqueue(*account_id).await.unwrap();
    }
    let clock = harness.clock.clone();
    harness
        .repository
        .on_activate(move || clock.advance(StdDuration::from_secs(1)));
    let spec = harness.runtime.only_spec();
    harness.join_all(&accounts, &spec.world_name).await;

    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Disconnected {
            connection_id: connection_id(0),
            account_id: accounts[0],
            observed_at: StdDuration::ZERO,
            world_name: Some(spec.world_name.clone()),
            world_generation: Some(TEST_WORLD_GENERATION.to_owned()),
            client_id: Some(public_player_id(&spec, accounts[0])),
            attach_attempt_id: Some("join-attempt-0".to_owned()),
        })
        .await
        .unwrap();

    assert_eq!(
        harness.repository.only_match().record.state,
        MatchState::Aborted
    );
    assert_eq!(harness.runtime.stop_count(), 1);
}

#[tokio::test]
async fn lobby_disconnect_observed_during_activation_does_not_abort_match_socket() {
    let harness = Harness::new().await;
    let accounts = accounts(10);
    harness.connect_all(&accounts).await;
    for account_id in &accounts {
        harness.service.enqueue(*account_id).await.unwrap();
    }
    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Connected {
            connection_id: "account-0-lobby".to_owned(),
            account_id: accounts[0],
        })
        .await
        .unwrap();
    let observed_at = harness.clock.monotonic_now();
    let clock = harness.clock.clone();
    harness
        .repository
        .on_activate(move || clock.advance(StdDuration::from_secs(1)));
    let spec = harness.runtime.only_spec();
    harness.join_all(&accounts, &spec.world_name).await;

    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Disconnected {
            connection_id: "account-0-lobby".to_owned(),
            account_id: accounts[0],
            observed_at,
            world_name: None,
            world_generation: None,
            client_id: None,
            attach_attempt_id: None,
        })
        .await
        .unwrap();

    assert_eq!(harness.runtime.stop_count(), 0);
    assert_eq!(
        harness.repository.only_match().record.state,
        MatchState::Active
    );
}

#[tokio::test]
async fn rebind_admitted_before_deadline_commits_after_deadline() {
    let harness = Harness::new().await;
    let accounts = accounts(10);
    harness.connect_all(&accounts).await;
    for account_id in &accounts {
        harness.service.enqueue(*account_id).await.unwrap();
    }
    let spec = harness.runtime.only_spec();
    harness.join_all(&accounts, &spec.world_name).await;
    let client_id = public_player_id(&spec, accounts[0]);
    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Disconnected {
            connection_id: connection_id(0),
            account_id: accounts[0],
            observed_at: harness.clock.monotonic_now(),
            world_name: Some(spec.world_name.clone()),
            world_generation: Some(TEST_WORLD_GENERATION.to_owned()),
            client_id: Some(client_id.clone()),
            attach_attempt_id: Some("join-attempt-0".to_owned()),
        })
        .await
        .unwrap();

    harness.clock.advance(StdDuration::from_millis(59_999));
    assert!(harness.service.allows_attach(
        &spec.world_name,
        TEST_WORLD_GENERATION,
        &client_id,
        "rebind-attempt-delayed",
        accounts[0],
        MatchAttachKind::Rebind,
    ));
    harness.clock.advance(StdDuration::from_millis(1));
    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Rebound {
            connection_id: "connection-0-rebound".to_owned(),
            account_id: accounts[0],
            world_name: spec.world_name.clone(),
            world_generation: TEST_WORLD_GENERATION.to_owned(),
            client_id: client_id.clone(),
            attach_attempt_id: "rebind-attempt-delayed".to_owned(),
        })
        .await
        .unwrap();
    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Rebound {
            connection_id: "connection-0-rebound".to_owned(),
            account_id: accounts[0],
            world_name: spec.world_name.clone(),
            world_generation: TEST_WORLD_GENERATION.to_owned(),
            client_id,
            attach_attempt_id: "rebind-attempt-delayed".to_owned(),
        })
        .await
        .unwrap();
    harness.service.tick().await.unwrap();

    let stored = harness.repository.only_match();
    assert_eq!(stored.participants[0].state, ParticipantState::Active);
    assert_eq!(harness.runtime.despawn_count(accounts[0]), 0);
}

#[tokio::test]
async fn timeout_failure_does_not_strand_other_expired_participants() {
    let harness = Harness::new().await;
    let accounts = accounts(10);
    harness.connect_all(&accounts).await;
    for account_id in &accounts {
        harness.service.enqueue(*account_id).await.unwrap();
    }
    let spec = harness.runtime.only_spec();
    harness.join_all(&accounts, &spec.world_name).await;
    for (index, account_id) in accounts.iter().copied().enumerate().take(2) {
        harness
            .service
            .apply_connection_event(MatchConnectionEvent::Disconnected {
                connection_id: connection_id(index),
                account_id,
                observed_at: harness.clock.monotonic_now(),
                world_name: Some(spec.world_name.clone()),
                world_generation: Some(TEST_WORLD_GENERATION.to_owned()),
                client_id: Some(public_player_id(&spec, account_id)),
                attach_attempt_id: Some(format!("join-attempt-{index}")),
            })
            .await
            .unwrap();
    }

    harness.clock.advance(StdDuration::from_secs(60));
    harness.repository.fail_next_time_out();
    assert_eq!(
        harness.service.tick().await,
        Err(MatchmakingError::Unavailable)
    );
    harness.service.tick().await.unwrap();

    let stored = harness.repository.only_match();
    for account_id in accounts.iter().take(2) {
        let participant = stored
            .participants
            .iter()
            .find(|participant| participant.account_id == *account_id)
            .unwrap();
        assert_eq!(participant.state, ParticipantState::TimedOut);
        assert_eq!(harness.runtime.despawn_count(*account_id), 1);
    }
}

#[tokio::test]
async fn reconnect_boundary_and_absolute_match_deadlines_are_exact() {
    let harness = Harness::new().await;
    let accounts = accounts(10);
    harness.connect_all(&accounts).await;
    for account_id in &accounts {
        harness.service.enqueue(*account_id).await.unwrap();
    }
    let spec = harness.runtime.only_spec();
    harness.join_all(&accounts, &spec.world_name).await;
    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Connected {
            connection_id: "connection-0-backup".to_owned(),
            account_id: accounts[0],
        })
        .await
        .unwrap();

    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Disconnected {
            connection_id: connection_id(0),
            account_id: accounts[0],
            observed_at: harness.clock.monotonic_now(),
            world_name: Some(spec.world_name.clone()),
            world_generation: Some(TEST_WORLD_GENERATION.to_owned()),
            client_id: Some(public_player_id(&spec, accounts[0])),
            attach_attempt_id: Some("join-attempt-0".to_owned()),
        })
        .await
        .unwrap();
    harness.clock.advance(StdDuration::from_secs(30));
    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Disconnected {
            connection_id: "connection-0-backup".to_owned(),
            account_id: accounts[0],
            observed_at: harness.clock.monotonic_now(),
            world_name: None,
            world_generation: None,
            client_id: None,
            attach_attempt_id: None,
        })
        .await
        .unwrap();
    harness.clock.advance(StdDuration::from_millis(29_999));
    assert!(harness.service.allows_attach(
        &spec.world_name,
        TEST_WORLD_GENERATION,
        &public_player_id(&spec, accounts[0]),
        "rebind-attempt-boundary",
        accounts[0],
        MatchAttachKind::Rebind,
    ));
    harness.clock.advance(StdDuration::from_millis(1));
    assert!(!harness.service.allows_attach(
        &spec.world_name,
        TEST_WORLD_GENERATION,
        &public_player_id(&spec, accounts[0]),
        "rebind-attempt-expired",
        accounts[0],
        MatchAttachKind::Rebind,
    ));
    harness
        .service
        .apply_connection_event(MatchConnectionEvent::RebindRejected {
            account_id: accounts[0],
            world_name: spec.world_name.clone(),
            world_generation: TEST_WORLD_GENERATION.to_owned(),
            client_id: public_player_id(&spec, accounts[0]),
            attach_attempt_id: "rebind-attempt-boundary".to_owned(),
        })
        .await
        .unwrap();
    harness.service.tick().await.unwrap();
    harness.service.tick().await.unwrap();
    assert_eq!(harness.runtime.despawn_count(accounts[0]), 1);

    harness.clock.set(StdDuration::from_secs(8 * 60));
    harness.service.tick().await.unwrap();
    assert_eq!(
        harness.service.enqueue(accounts[1]).await.unwrap().status,
        QueueStatus::ExtractionOpen
    );
    harness.clock.set(StdDuration::from_secs(12 * 60));
    harness.service.tick().await.unwrap();
    assert_eq!(harness.runtime.stop_count(), 1);
    assert_eq!(
        harness.repository.only_match().record.state,
        MatchState::Finished
    );

    harness
        .service
        .apply_connection_event(MatchConnectionEvent::Connected {
            connection_id: "connection-0-next-match".to_owned(),
            account_id: accounts[0],
        })
        .await
        .unwrap();
    let next = harness.service.enqueue(accounts[0]).await.unwrap();
    assert_eq!(next.status, QueueStatus::Queued);
    assert_eq!(next.position, Some(1));
}

#[tokio::test]
async fn memory_startup_recovery_aborts_nonterminal_state_once() {
    let harness = Harness::new().await;
    let accounts = accounts(MATCH_SIZE);
    harness.connect_all(&accounts).await;
    for account_id in &accounts {
        harness.service.enqueue(*account_id).await.unwrap();
    }

    {
        let mut matches = harness.repository.matches.lock().unwrap();
        let stored = matches.values_mut().next().unwrap();
        stored.participants[0].state = ParticipantState::Dead;
    }
    let recovered_at: OffsetDateTime = harness.clock.utc_now().into();
    assert_eq!(
        harness
            .repository
            .abort_unrecoverable_matches("process_restart".to_owned(), recovered_at)
            .await,
        Ok(1)
    );

    let recovered = harness.repository.only_match();
    assert_eq!(recovered.record.state, MatchState::Aborted);
    assert_eq!(recovered.record.finished_at, Some(recovered_at));
    assert_eq!(
        recovered.record.abort_reason.as_deref(),
        Some("process_restart")
    );
    assert_eq!(recovered.participants[0].state, ParticipantState::Dead);
    assert!(recovered.participants[1..]
        .iter()
        .all(|participant| participant.state == ParticipantState::Aborted));
    assert_eq!(
        harness
            .repository
            .abort_unrecoverable_matches("process_restart".to_owned(), recovered_at)
            .await,
        Ok(0)
    );
}

struct Harness {
    service: Arc<MatchmakingService>,
    repository: Arc<MemoryMatchRepository>,
    runtime: Arc<MemoryWorldRuntime>,
    clock: Arc<ManualClock>,
}

impl Harness {
    async fn new() -> Self {
        let repository = Arc::new(MemoryMatchRepository::default());
        let runtime = Arc::new(MemoryWorldRuntime::default());
        let clock = Arc::new(ManualClock::default());
        let service = MatchmakingService::start(
            repository.clone(),
            clock.clone(),
            Arc::new(SequenceIds::default()),
            Arc::new(FixedSeed),
            MatchVersions {
                generation: "generation-v1".to_owned(),
                gameplay: "gameplay-v1".to_owned(),
                config: "config-v1".to_owned(),
            },
        );
        service.bind_runtime(runtime.clone()).await.unwrap();
        Self {
            service,
            repository,
            runtime,
            clock,
        }
    }

    async fn connect_all(&self, accounts: &[Uuid]) {
        for (index, account_id) in accounts.iter().enumerate() {
            self.service
                .apply_connection_event(MatchConnectionEvent::Connected {
                    connection_id: connection_id(index),
                    account_id: *account_id,
                })
                .await
                .unwrap();
        }
    }

    async fn join_all(&self, accounts: &[Uuid], world_name: &str) {
        let spec = self.runtime.only_spec();
        for (index, account_id) in accounts.iter().enumerate() {
            assert!(self.service.allows_attach(
                world_name,
                TEST_WORLD_GENERATION,
                &public_player_id(&spec, *account_id),
                &format!("join-attempt-{index}"),
                *account_id,
                MatchAttachKind::Join,
            ));
            self.service
                .apply_connection_event(MatchConnectionEvent::JoinCommitted {
                    connection_id: connection_id(index),
                    account_id: *account_id,
                    world_name: world_name.to_owned(),
                    world_generation: TEST_WORLD_GENERATION.to_owned(),
                    client_id: public_player_id(&spec, *account_id),
                    attach_attempt_id: format!("join-attempt-{index}"),
                })
                .await
                .unwrap();
        }
    }
}

fn accounts(count: usize) -> Vec<Uuid> {
    (0..count)
        .map(|index| Uuid::from_u128(10_000 + index as u128))
        .collect()
}

fn connection_id(index: usize) -> String {
    format!("connection-{index}")
}

fn public_player_id(spec: &MatchWorldSpec, account_id: Uuid) -> String {
    spec.roster
        .iter()
        .find(|participant| participant.account_id == account_id)
        .unwrap()
        .public_player_id
        .to_string()
}

#[derive(Default)]
struct ManualClock {
    millis: AtomicU64,
}

impl ManualClock {
    fn advance(&self, duration: StdDuration) {
        self.millis
            .fetch_add(duration.as_millis() as u64, Ordering::SeqCst);
    }

    fn set(&self, duration: StdDuration) {
        self.millis
            .store(duration.as_millis() as u64, Ordering::SeqCst);
    }
}

impl Clock for ManualClock {
    fn monotonic_now(&self) -> StdDuration {
        StdDuration::from_millis(self.millis.load(Ordering::SeqCst))
    }

    fn utc_now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH + StdDuration::from_secs(1_800_000_000) + self.monotonic_now()
    }
}

#[derive(Default)]
struct SequenceIds {
    next: AtomicU64,
}

impl IdGenerator for SequenceIds {
    fn next_uuid(&self) -> Uuid {
        Uuid::from_u128(100_000 + self.next.fetch_add(1, Ordering::SeqCst) as u128)
    }
}

struct FixedSeed;

impl SeedGenerator for FixedSeed {
    fn next_seed(&self) -> u64 {
        0x1122_3344_5566_7788
    }
}

#[derive(Default)]
struct MemoryWorldRuntime {
    specs: Mutex<Vec<MatchWorldSpec>>,
    stops: Mutex<Vec<String>>,
    despawns: Mutex<HashMap<Uuid, usize>>,
    hang_stop: AtomicBool,
}

impl MemoryWorldRuntime {
    fn only_spec(&self) -> MatchWorldSpec {
        let specs = self.specs.lock().unwrap();
        assert_eq!(specs.len(), 1);
        specs[0].clone()
    }

    fn stop_count(&self) -> usize {
        self.stops.lock().unwrap().len()
    }

    fn spec_count(&self) -> usize {
        self.specs.lock().unwrap().len()
    }

    fn despawn_count(&self, account_id: Uuid) -> usize {
        self.despawns
            .lock()
            .unwrap()
            .get(&account_id)
            .copied()
            .unwrap_or_default()
    }
}

#[async_trait]
impl MatchWorldRuntime for MemoryWorldRuntime {
    async fn prepare_world(
        &self,
        spec: MatchWorldSpec,
    ) -> Result<PreparedMatchWorld, MatchWorldRuntimeError> {
        self.specs.lock().unwrap().push(spec);
        Ok(PreparedMatchWorld {
            world_generation: TEST_WORLD_GENERATION.to_owned(),
        })
    }

    async fn stop_world(
        &self,
        _match_id: Uuid,
        world_name: &str,
    ) -> Result<bool, MatchWorldRuntimeError> {
        if self.hang_stop.load(Ordering::SeqCst) {
            std::future::pending::<()>().await;
        }
        self.stops.lock().unwrap().push(world_name.to_owned());
        Ok(true)
    }

    async fn despawn_detached(
        &self,
        _world_name: &str,
        account_id: Uuid,
    ) -> Result<bool, MatchWorldRuntimeError> {
        *self.despawns.lock().unwrap().entry(account_id).or_default() += 1;
        Ok(true)
    }
}

#[derive(Default)]
struct MemoryMatchRepository {
    matches: Mutex<HashMap<Uuid, StoredMatch>>,
    activate_hook: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
    fail_create_after_store: AtomicBool,
    fail_begin_settling: AtomicBool,
    fail_time_out: AtomicBool,
}

impl MemoryMatchRepository {
    fn only_match(&self) -> StoredMatch {
        let matches = self.matches.lock().unwrap();
        assert_eq!(matches.len(), 1);
        matches.values().next().unwrap().clone()
    }

    fn on_activate(&self, hook: impl Fn() + Send + Sync + 'static) {
        *self.activate_hook.lock().unwrap() = Some(Arc::new(hook));
    }

    fn fail_next_create_after_store(&self) {
        self.fail_create_after_store.store(true, Ordering::SeqCst);
    }

    fn fail_next_begin_settling(&self) {
        self.fail_begin_settling.store(true, Ordering::SeqCst);
    }

    fn fail_next_time_out(&self) {
        self.fail_time_out.store(true, Ordering::SeqCst);
    }

    fn match_count(&self) -> usize {
        self.matches.lock().unwrap().len()
    }

    fn match_by_id(&self, match_id: Uuid) -> StoredMatch {
        self.matches.lock().unwrap()[&match_id].clone()
    }

    fn update<T>(
        &self,
        match_id: Uuid,
        apply: impl FnOnce(&mut StoredMatch) -> Result<T, MatchRepositoryError>,
    ) -> Result<TransitionOutcome<T>, MatchRepositoryError> {
        let mut matches = self.matches.lock().unwrap();
        let stored = matches
            .get_mut(&match_id)
            .ok_or(MatchRepositoryError::Conflict)?;
        apply(stored).map(TransitionOutcome::Applied)
    }
}

impl RepositoryProbe for MemoryMatchRepository {
    fn check(&self) -> RepositoryFuture<'_> {
        Box::pin(async { Ok(()) })
    }
}

#[async_trait]
impl MatchRepository for MemoryMatchRepository {
    async fn create_preparing(
        &self,
        command: CreatePreparingMatch,
    ) -> Result<StoredMatch, MatchRepositoryError> {
        let participants = command
            .roster
            .iter()
            .map(|seat| ParticipantRecord {
                match_id: command.match_id,
                account_id: seat.account_id,
                public_player_id: seat.public_player_id,
                seat_id: seat.seat_id,
                state: ParticipantState::Preparing,
                enqueued_at: seat.enqueued_at,
                reconnect_deadline: None,
                killed_by_account_id: None,
                extracted_at: None,
                settlement_qualified_at: None,
            })
            .collect::<Vec<_>>();
        let stored = StoredMatch {
            record: MatchRecord {
                match_id: command.match_id,
                state: MatchState::Preparing,
                world_name: command.world_name,
                seed: command.seed,
                versions: command.versions,
                created_at: command.created_at,
                started_at: None,
                extraction_open_at: None,
                hard_deadline: None,
                settlement_grace_deadline: None,
                finished_at: None,
                abort_reason: None,
            },
            participants,
        };
        self.matches
            .lock()
            .unwrap()
            .insert(command.match_id, stored.clone());
        if self.fail_create_after_store.swap(false, Ordering::SeqCst) {
            return Err(MatchRepositoryError::Unavailable);
        }
        Ok(stored)
    }

    async fn find_match(
        &self,
        match_id: Uuid,
    ) -> Result<Option<StoredMatch>, MatchRepositoryError> {
        Ok(self.matches.lock().unwrap().get(&match_id).cloned())
    }

    async fn find_nonterminal_by_account(
        &self,
        account_id: Uuid,
    ) -> Result<Option<StoredMatch>, MatchRepositoryError> {
        Ok(self
            .matches
            .lock()
            .unwrap()
            .values()
            .find(|stored| {
                stored.participants.iter().any(|participant| {
                    participant.account_id == account_id
                        && participant.state.occupies_nonterminal_seat()
                })
            })
            .cloned())
    }

    async fn abort_unrecoverable_matches(
        &self,
        reason: String,
        at: OffsetDateTime,
    ) -> Result<u64, MatchRepositoryError> {
        if reason.trim().is_empty() {
            return Err(MatchRepositoryError::Conflict);
        }
        let mut matches = self.matches.lock().unwrap();
        let mut aborted = 0;
        for stored in matches.values_mut() {
            if stored.record.state.is_terminal() {
                continue;
            }
            stored.record.state = MatchState::Aborted;
            stored.record.abort_reason = Some(reason.clone());
            stored.record.finished_at = Some(at.max(stored.record.created_at));
            for participant in &mut stored.participants {
                if !participant.state.is_terminal() {
                    participant.state = ParticipantState::Aborted;
                    participant.reconnect_deadline = None;
                }
            }
            aborted += 1;
        }
        Ok(aborted)
    }

    async fn activate(
        &self,
        match_id: Uuid,
        started_at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
        if let Some(hook) = self.activate_hook.lock().unwrap().take() {
            hook();
        }
        self.update(match_id, |stored| {
            let deadlines = ActivationDeadlines::from_started_at(started_at)
                .ok_or(MatchRepositoryError::Conflict)?;
            stored.record.state = MatchState::Active;
            stored.record.started_at = Some(started_at);
            stored.record.extraction_open_at = Some(deadlines.extraction_open_at);
            stored.record.hard_deadline = Some(deadlines.hard_deadline);
            stored.record.settlement_grace_deadline = Some(deadlines.settlement_grace_deadline);
            for participant in &mut stored.participants {
                participant.state = ParticipantState::Active;
            }
            Ok(stored.clone())
        })
    }

    async fn abort(
        &self,
        match_id: Uuid,
        reason: String,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
        self.update(match_id, |stored| {
            stored.record.state = MatchState::Aborted;
            stored.record.abort_reason = Some(reason);
            stored.record.finished_at = Some(at);
            for participant in &mut stored.participants {
                if !participant.state.is_terminal() {
                    participant.state = ParticipantState::Aborted;
                }
            }
            Ok(stored.clone())
        })
    }

    async fn mark_disconnected(
        &self,
        match_id: Uuid,
        account_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError> {
        self.update(match_id, |stored| {
            let participant = participant_mut(stored, account_id)?;
            if participant.state == ParticipantState::Active {
                participant.state = ParticipantState::Disconnected;
                participant.reconnect_deadline = Some(at + RECONNECT_WINDOW);
            }
            Ok(participant.clone())
        })
    }

    async fn reconnect(
        &self,
        match_id: Uuid,
        account_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError> {
        self.update(match_id, |stored| {
            let participant = participant_mut(stored, account_id)?;
            if participant.state != ParticipantState::Disconnected
                || participant
                    .reconnect_deadline
                    .is_none_or(|deadline| at >= deadline)
            {
                return Err(MatchRepositoryError::Conflict);
            }
            participant.state = ParticipantState::Active;
            participant.reconnect_deadline = None;
            Ok(participant.clone())
        })
    }

    async fn time_out(
        &self,
        match_id: Uuid,
        account_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<ParticipantRecord>, MatchRepositoryError> {
        if self.fail_time_out.swap(false, Ordering::SeqCst) {
            return Err(MatchRepositoryError::Unavailable);
        }
        self.update(match_id, |stored| {
            let participant = participant_mut(stored, account_id)?;
            if participant.state == ParticipantState::Disconnected
                && participant
                    .reconnect_deadline
                    .is_some_and(|deadline| at >= deadline)
            {
                participant.state = ParticipantState::TimedOut;
                participant.reconnect_deadline = None;
            }
            Ok(participant.clone())
        })
    }

    async fn open_extraction(
        &self,
        match_id: Uuid,
        _at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
        self.update(match_id, |stored| {
            stored.record.state = MatchState::ExtractionOpen;
            Ok(stored.clone())
        })
    }

    async fn begin_settling(
        &self,
        match_id: Uuid,
        trigger: SettlingTrigger,
        _at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
        if self.fail_begin_settling.swap(false, Ordering::SeqCst) {
            return Err(MatchRepositoryError::Unavailable);
        }
        self.update(match_id, |stored| {
            if trigger == SettlingTrigger::HardDeadline {
                for participant in &mut stored.participants {
                    if matches!(
                        participant.state,
                        ParticipantState::Active | ParticipantState::Disconnected
                    ) {
                        participant.state = ParticipantState::TimedOut;
                        participant.reconnect_deadline = None;
                    }
                }
            }
            stored.record.state = MatchState::Settling;
            Ok(stored.clone())
        })
    }

    async fn finish(
        &self,
        match_id: Uuid,
        at: OffsetDateTime,
    ) -> Result<TransitionOutcome<StoredMatch>, MatchRepositoryError> {
        self.update(match_id, |stored| {
            stored.record.state = MatchState::Finished;
            stored.record.finished_at = Some(at);
            Ok(stored.clone())
        })
    }
}

fn participant_mut(
    stored: &mut StoredMatch,
    account_id: Uuid,
) -> Result<&mut ParticipantRecord, MatchRepositoryError> {
    stored
        .participants
        .iter_mut()
        .find(|participant| participant.account_id == account_id)
        .ok_or(MatchRepositoryError::Conflict)
}
