#![cfg(feature = "db-tests")]

use std::env;

use extraction_server::{
    matchmaking::{
        CreatePreparingMatch, FrozenRoster, MatchState, MatchVersions, ParticipantDeath,
        ParticipantMatchStats, ParticipantResourceCounts, ParticipantState, ParticipantTimeout,
        QueuedPlayer, MATCH_SIZE,
    },
    persistence::{acquire_matchmaking_process_lock, migrate_database, PgRepository},
    ports::{MatchRepository, MatchRepositoryError, SettlingTrigger, TransitionOutcome},
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

static MATCHMAKING_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[actix_web::test]
async fn process_lock_rejects_a_second_matchmaking_instance() {
    let _guard = MATCHMAKING_TEST_LOCK.lock().await;
    let database_url = test_database_url();
    let first = acquire_matchmaking_process_lock(&database_url)
        .await
        .unwrap()
        .expect("第一个 matchmaking 进程应取得数据库锁");
    assert!(acquire_matchmaking_process_lock(&database_url)
        .await
        .unwrap()
        .is_none());
    drop(first);

    let mut reacquired = None;
    for _ in 0..20 {
        reacquired = acquire_matchmaking_process_lock(&database_url)
            .await
            .unwrap();
        if reacquired.is_some() {
            break;
        }
        actix_web::rt::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(reacquired.is_some(), "持锁连接关闭后必须释放进程锁");
}

#[actix_web::test]
async fn preparing_creation_persists_exactly_ten_fifo_seats_atomically() {
    let _guard = MATCHMAKING_TEST_LOCK.lock().await;
    let repository = test_repository().await;
    let accounts = insert_accounts(&repository, MATCH_SIZE).await;
    let created_at = OffsetDateTime::now_utc();
    let command = preparing_command(&accounts, created_at);
    let retry_command = command.clone();
    let match_id = command.match_id;

    let created = repository
        .create_preparing(command)
        .await
        .expect("恰好 10 人名单应原子写入");
    let retried = repository
        .create_preparing(retry_command)
        .await
        .expect("相同 prepare attempt 重试应读回既有记录");
    assert_eq!(retried, created);
    assert_eq!(created.record.state, MatchState::Preparing);
    assert_eq!(created.participants.len(), MATCH_SIZE);
    for (index, participant) in created.participants.iter().enumerate() {
        assert_eq!(participant.account_id, accounts[index]);
        assert_eq!(usize::from(participant.seat_id.get()), index);
        assert_eq!(participant.state, ParticipantState::Preparing);
        assert_eq!(
            participant.enqueued_at,
            created_at - Duration::minutes(1) + Duration::seconds(index as i64)
        );
    }

    let audited = repository
        .find_match(match_id)
        .await
        .expect("应能审计比赛及全部 participant")
        .expect("比赛应存在");
    assert_eq!(audited, created);
    assert_eq!(
        repository
            .find_nonterminal_by_account(accounts[5])
            .await
            .unwrap(),
        Some(created)
    );
}

#[actix_web::test]
async fn overlapping_rosters_allow_only_one_nonterminal_seat() {
    let _guard = MATCHMAKING_TEST_LOCK.lock().await;
    let repository = test_repository().await;
    let accounts = insert_accounts(&repository, MATCH_SIZE * 2 - 2).await;
    let created_at = OffsetDateTime::now_utc();
    let first_accounts = accounts[..MATCH_SIZE].to_vec();
    let mut second_accounts = vec![accounts[1], accounts[0]];
    second_accounts.extend_from_slice(&accounts[MATCH_SIZE..]);
    let first = preparing_command(&first_accounts, created_at);
    let second = preparing_command(&second_accounts, created_at);
    let first_match_id = first.match_id;
    let second_match_id = second.match_id;

    let first_attempt = {
        let repository = repository.clone();
        actix_web::rt::spawn(async move { repository.create_preparing(first).await })
    };
    let second_attempt = {
        let repository = repository.clone();
        actix_web::rt::spawn(async move { repository.create_preparing(second).await })
    };
    let results = actix_web::rt::time::timeout(std::time::Duration::from_secs(5), async {
        [
            first_attempt.await.expect("首个创建任务不应崩溃"),
            second_attempt.await.expect("第二个创建任务不应崩溃"),
        ]
    })
    .await
    .expect("反序重叠名单不应形成账号锁死锁");
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| **result == Err(MatchRepositoryError::SeatOccupied))
            .count(),
        1
    );

    let persisted = [first_match_id, second_match_id]
        .into_iter()
        .map(|match_id| {
            let repository = repository.clone();
            async move { repository.find_match(match_id).await.unwrap().is_some() }
        });
    let mut persisted_count = 0;
    for lookup in persisted {
        persisted_count += usize::from(lookup.await);
    }
    assert_eq!(persisted_count, 1, "失败事务不得留下部分比赛记录");
}

#[actix_web::test]
async fn lifecycle_deadlines_reconnect_and_hard_timeout_are_cas_guarded() {
    let _guard = MATCHMAKING_TEST_LOCK.lock().await;
    let repository = test_repository().await;
    let accounts = insert_accounts(&repository, MATCH_SIZE).await;
    let created_at = OffsetDateTime::now_utc();
    let command = preparing_command(&accounts, created_at);
    let match_id = command.match_id;
    repository.create_preparing(command).await.unwrap();

    let started_at = created_at + Duration::seconds(1);
    let active = repository
        .activate(match_id, started_at)
        .await
        .expect("Preparing 应激活")
        .into_value();
    assert_eq!(active.record.state, MatchState::Active);
    assert_eq!(
        active.record.extraction_open_at,
        Some(started_at + Duration::minutes(8))
    );
    assert_eq!(
        active.record.hard_deadline,
        Some(started_at + Duration::minutes(12))
    );
    assert_eq!(
        active.record.settlement_grace_deadline,
        Some(started_at + Duration::minutes(12) + Duration::seconds(30))
    );

    let disconnected_at = started_at + Duration::seconds(10);
    let disconnected = repository
        .mark_disconnected(match_id, accounts[0], disconnected_at)
        .await
        .unwrap();
    let reconnect_deadline = disconnected.value().reconnect_deadline.unwrap();
    assert_eq!(reconnect_deadline, disconnected_at + Duration::seconds(60));
    let duplicate = repository
        .mark_disconnected(
            match_id,
            accounts[0],
            disconnected_at + Duration::seconds(20),
        )
        .await
        .unwrap();
    assert!(matches!(duplicate, TransitionOutcome::AlreadyApplied(_)));
    assert_eq!(
        duplicate.value().reconnect_deadline,
        Some(reconnect_deadline)
    );
    assert!(repository
        .reconnect(
            match_id,
            accounts[0],
            reconnect_deadline - Duration::nanoseconds(1)
        )
        .await
        .unwrap()
        .was_applied());

    let disconnected_again_at = started_at + Duration::seconds(80);
    let deadline = repository
        .mark_disconnected(match_id, accounts[0], disconnected_again_at)
        .await
        .unwrap()
        .into_value()
        .reconnect_deadline
        .unwrap();
    assert_eq!(
        repository.reconnect(match_id, accounts[0], deadline).await,
        Err(MatchRepositoryError::Conflict)
    );
    assert!(repository
        .mark_timed_out(
            ParticipantTimeout {
                match_id,
                account_id: accounts[0],
                stats: ParticipantMatchStats::default(),
            },
            deadline,
        )
        .await
        .unwrap()
        .was_applied());
    assert!(!repository
        .mark_timed_out(
            ParticipantTimeout {
                match_id,
                account_id: accounts[0],
                stats: ParticipantMatchStats::default(),
            },
            deadline + Duration::seconds(1),
        )
        .await
        .unwrap()
        .was_applied());

    assert_eq!(
        repository
            .begin_settling(
                match_id,
                SettlingTrigger::AllParticipantsTerminal,
                started_at + Duration::minutes(1),
            )
            .await,
        Err(MatchRepositoryError::Conflict)
    );
    let extraction_at = started_at + Duration::minutes(8);
    assert_eq!(
        repository
            .open_extraction(match_id, extraction_at - Duration::nanoseconds(1))
            .await,
        Err(MatchRepositoryError::Conflict)
    );
    assert!(repository
        .open_extraction(match_id, extraction_at)
        .await
        .unwrap()
        .was_applied());

    let hard_deadline = started_at + Duration::minutes(12);
    let settling = repository
        .begin_settling(match_id, SettlingTrigger::HardDeadline, hard_deadline)
        .await
        .unwrap()
        .into_value();
    assert_eq!(settling.record.state, MatchState::Settling);
    assert!(settling
        .participants
        .iter()
        .all(|participant| participant.state == ParticipantState::TimedOut));
    assert!(repository
        .finish(match_id, hard_deadline + Duration::seconds(1))
        .await
        .unwrap()
        .was_applied());
    assert!(repository
        .find_nonterminal_by_account(accounts[0])
        .await
        .unwrap()
        .is_none());
}

#[actix_web::test]
async fn participant_death_is_an_exact_idempotent_cas() {
    let _guard = MATCHMAKING_TEST_LOCK.lock().await;
    let repository = test_repository().await;
    let accounts = insert_accounts(&repository, MATCH_SIZE).await;
    let created_at = OffsetDateTime::now_utc();
    let command = preparing_command(&accounts, created_at);
    let match_id = command.match_id;
    repository.create_preparing(command).await.unwrap();
    repository
        .activate(match_id, created_at + Duration::seconds(1))
        .await
        .unwrap();
    let stats = ParticipantMatchStats {
        mined: ParticipantResourceCounts {
            dirt: 12,
            gold: 3,
            diamond: 1,
        },
        picked_up: ParticipantResourceCounts {
            dirt: 2,
            gold: 5,
            diamond: 1,
        },
        lost: ParticipantResourceCounts {
            dirt: 14,
            gold: 8,
            diamond: 2,
        },
    };
    let active_death = ParticipantDeath {
        match_id,
        victim_account_id: accounts[0],
        killer_account_id: accounts[1],
        stats,
    };

    let applied = repository.mark_dead(active_death.clone()).await.unwrap();
    assert!(applied.was_applied());
    assert_eq!(applied.value().state, ParticipantState::Dead);
    assert_eq!(applied.value().killed_by_account_id, Some(accounts[1]));
    assert_eq!(applied.value().stats, stats);
    let persisted = repository.find_match(match_id).await.unwrap().unwrap();
    let persisted_victim = persisted
        .participants
        .iter()
        .find(|participant| participant.account_id == accounts[0])
        .unwrap();
    assert_eq!(persisted_victim.state, ParticipantState::Dead);
    assert_eq!(persisted_victim.killed_by_account_id, Some(accounts[1]));
    assert_eq!(persisted_victim.stats, stats);
    assert!(persisted_victim.reconnect_deadline.is_none());
    assert!(matches!(
        repository.mark_dead(active_death.clone()).await.unwrap(),
        TransitionOutcome::AlreadyApplied(_)
    ));
    assert_eq!(
        repository
            .mark_dead(ParticipantDeath {
                killer_account_id: accounts[2],
                ..active_death.clone()
            })
            .await,
        Err(MatchRepositoryError::Conflict)
    );
    assert_eq!(
        repository
            .mark_timed_out(
                ParticipantTimeout {
                    match_id,
                    account_id: accounts[0],
                    stats,
                },
                created_at + Duration::minutes(2),
            )
            .await,
        Err(MatchRepositoryError::Conflict)
    );

    repository
        .mark_disconnected(match_id, accounts[3], created_at + Duration::seconds(2))
        .await
        .unwrap();
    let disconnected_death = ParticipantDeath {
        match_id,
        victim_account_id: accounts[3],
        killer_account_id: accounts[4],
        stats,
    };
    assert!(repository
        .mark_dead(disconnected_death)
        .await
        .unwrap()
        .was_applied());
}

#[actix_web::test]
async fn participant_timeout_persists_stats_and_excludes_death_terminal() {
    let _guard = MATCHMAKING_TEST_LOCK.lock().await;
    let repository = test_repository().await;
    let accounts = insert_accounts(&repository, MATCH_SIZE).await;
    let created_at = OffsetDateTime::now_utc();
    let command = preparing_command(&accounts, created_at);
    let match_id = command.match_id;
    repository.create_preparing(command).await.unwrap();
    repository
        .activate(match_id, created_at + Duration::seconds(1))
        .await
        .unwrap();
    let deadline = repository
        .mark_disconnected(match_id, accounts[0], created_at + Duration::seconds(2))
        .await
        .unwrap()
        .into_value()
        .reconnect_deadline
        .unwrap();
    let stats = ParticipantMatchStats {
        mined: ParticipantResourceCounts {
            dirt: 21,
            gold: 8,
            diamond: 3,
        },
        picked_up: ParticipantResourceCounts {
            dirt: 5,
            gold: 13,
            diamond: 2,
        },
        lost: ParticipantResourceCounts {
            dirt: 26,
            gold: 21,
            diamond: 5,
        },
    };
    let timeout = ParticipantTimeout {
        match_id,
        account_id: accounts[0],
        stats,
    };

    assert_eq!(
        repository
            .mark_timed_out(timeout.clone(), deadline - Duration::nanoseconds(1))
            .await,
        Err(MatchRepositoryError::Conflict)
    );
    let applied = repository
        .mark_timed_out(timeout.clone(), deadline)
        .await
        .unwrap();
    assert!(applied.was_applied());
    assert_eq!(applied.value().state, ParticipantState::TimedOut);
    assert_eq!(applied.value().stats, stats);
    assert!(applied.value().killed_by_account_id.is_none());
    assert!(applied.value().reconnect_deadline.is_none());

    let persisted = repository.find_match(match_id).await.unwrap().unwrap();
    let persisted_timeout = persisted
        .participants
        .iter()
        .find(|participant| participant.account_id == accounts[0])
        .unwrap();
    assert_eq!(persisted_timeout.state, ParticipantState::TimedOut);
    assert_eq!(persisted_timeout.stats, stats);
    assert!(persisted_timeout.killed_by_account_id.is_none());
    assert!(persisted_timeout.reconnect_deadline.is_none());
    assert!(matches!(
        repository
            .mark_timed_out(timeout.clone(), deadline + Duration::seconds(1))
            .await
            .unwrap(),
        TransitionOutcome::AlreadyApplied(_)
    ));

    let mut conflicting_timeout = timeout;
    conflicting_timeout.stats.lost.diamond += 1;
    assert_eq!(
        repository
            .mark_timed_out(conflicting_timeout, deadline + Duration::seconds(1))
            .await,
        Err(MatchRepositoryError::Conflict)
    );
    assert_eq!(
        repository
            .mark_dead(ParticipantDeath {
                match_id,
                victim_account_id: accounts[0],
                killer_account_id: accounts[1],
                stats,
            })
            .await,
        Err(MatchRepositoryError::Conflict)
    );
    let unchanged = repository.find_match(match_id).await.unwrap().unwrap();
    let unchanged_timeout = unchanged
        .participants
        .iter()
        .find(|participant| participant.account_id == accounts[0])
        .unwrap();
    assert_eq!(unchanged_timeout.state, ParticipantState::TimedOut);
    assert_eq!(unchanged_timeout.stats, stats);
    assert!(unchanged_timeout.killed_by_account_id.is_none());
}

#[actix_web::test]
async fn preparing_abort_is_atomic_and_releases_every_account() {
    let _guard = MATCHMAKING_TEST_LOCK.lock().await;
    let repository = test_repository().await;
    let accounts = insert_accounts(&repository, MATCH_SIZE).await;
    let created_at = OffsetDateTime::now_utc();
    let first = preparing_command(&accounts, created_at);
    let first_id = first.match_id;
    repository.create_preparing(first).await.unwrap();

    let aborted = repository
        .abort(
            first_id,
            "preparing participant disconnected".to_owned(),
            created_at + Duration::seconds(1),
        )
        .await
        .unwrap()
        .into_value();
    assert_eq!(aborted.record.state, MatchState::Aborted);
    assert!(aborted
        .participants
        .iter()
        .all(|participant| participant.state == ParticipantState::Aborted));
    for account_id in &accounts {
        assert!(repository
            .find_nonterminal_by_account(*account_id)
            .await
            .unwrap()
            .is_none());
    }

    let replacement = preparing_command(&accounts, created_at + Duration::seconds(2));
    assert!(repository.create_preparing(replacement).await.is_ok());
}

#[actix_web::test]
async fn startup_recovery_atomically_aborts_all_nonterminal_matches() {
    let _guard = MATCHMAKING_TEST_LOCK.lock().await;
    let repository = test_repository().await;
    let cleanup_at = OffsetDateTime::now_utc();
    repository
        .abort_unrecoverable_matches("test_cleanup".to_owned(), cleanup_at)
        .await
        .expect("测试前应能收敛遗留比赛");

    let accounts = insert_accounts(&repository, MATCH_SIZE * 2).await;
    let created_at = cleanup_at + Duration::seconds(1);
    let active_command = preparing_command(&accounts[..MATCH_SIZE], created_at);
    let active_match_id = active_command.match_id;
    repository.create_preparing(active_command).await.unwrap();
    repository
        .activate(active_match_id, created_at + Duration::seconds(1))
        .await
        .unwrap();
    sqlx::query(
        "UPDATE match_participants SET state = 'extracted', extracted_at = $3 \
         WHERE match_id = $1 AND account_id = $2",
    )
    .bind(active_match_id)
    .bind(accounts[0])
    .bind(created_at + Duration::seconds(2))
    .execute(repository.pool())
    .await
    .expect("测试应能模拟已经提交完成的终态参与者");
    let settlement_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO extraction_settlements (\
           id, match_id, account_id, idempotency_key, inventory_digest, config_version, \
           total_value, committed_at\
         ) VALUES ($1, $2, $3, $4, $5, 'config-v1', 0, $6)",
    )
    .bind(settlement_id)
    .bind(active_match_id)
    .bind(accounts[0])
    .bind(format!("extract:v1:{active_match_id}:{}", accounts[0]))
    .bind(vec![0_u8; 32])
    .bind(created_at + Duration::seconds(2))
    .execute(repository.pool())
    .await
    .expect("测试应能模拟已提交的唯一结算");

    let preparing_command = preparing_command(&accounts[MATCH_SIZE..], created_at);
    let preparing_match_id = preparing_command.match_id;
    repository
        .create_preparing(preparing_command)
        .await
        .unwrap();

    let recovered_at = created_at + Duration::seconds(3);
    assert_eq!(
        repository
            .abort_unrecoverable_matches("process_restart".to_owned(), recovered_at)
            .await,
        Ok(2)
    );

    let active = repository
        .find_match(active_match_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(active.record.state, MatchState::Aborted);
    assert_eq!(active.record.finished_at, Some(recovered_at));
    assert_eq!(
        active.record.abort_reason.as_deref(),
        Some("process_restart")
    );
    assert_eq!(active.participants[0].state, ParticipantState::Extracted);
    assert!(active.participants[1..]
        .iter()
        .all(|participant| participant.state == ParticipantState::Aborted));
    let preserved_settlement =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM extraction_settlements WHERE id = $1")
            .bind(settlement_id)
            .fetch_one(repository.pool())
            .await
            .unwrap();
    assert_eq!(preserved_settlement, 1, "恢复不得删除已提交结算");

    let preparing = repository
        .find_match(preparing_match_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(preparing.record.state, MatchState::Aborted);
    assert!(preparing
        .participants
        .iter()
        .all(|participant| participant.state == ParticipantState::Aborted));
    for account_id in &accounts {
        assert!(repository
            .find_nonterminal_by_account(*account_id)
            .await
            .unwrap()
            .is_none());
    }
    assert_eq!(
        repository
            .abort_unrecoverable_matches("process_restart".to_owned(), recovered_at)
            .await,
        Ok(0)
    );
}

#[actix_web::test]
async fn closed_pool_is_reported_as_unavailable() {
    let _guard = MATCHMAKING_TEST_LOCK.lock().await;
    let repository = test_repository().await;
    repository.pool().close().await;

    assert_eq!(
        repository.find_match(Uuid::new_v4()).await,
        Err(MatchRepositoryError::Unavailable)
    );
}

async fn test_repository() -> PgRepository {
    let database_url = test_database_url();
    migrate_database(&database_url)
        .await
        .expect("测试数据库迁移应成功");
    PgRepository::connect(&database_url)
        .await
        .expect("应能连接测试数据库")
}

async fn insert_accounts(repository: &PgRepository, count: usize) -> Vec<Uuid> {
    let now = OffsetDateTime::now_utc();
    let accounts = (0..count).map(|_| Uuid::new_v4()).collect::<Vec<_>>();
    for account_id in &accounts {
        sqlx::query("INSERT INTO accounts (id, created_at, updated_at) VALUES ($1, $2, $2)")
            .bind(account_id)
            .bind(now)
            .execute(repository.pool())
            .await
            .expect("测试账号应写入成功");
    }
    accounts
}

fn preparing_command(accounts: &[Uuid], created_at: OffsetDateTime) -> CreatePreparingMatch {
    let match_id = Uuid::new_v4();
    let queued_at = created_at - Duration::minutes(1);
    let players = accounts
        .iter()
        .enumerate()
        .map(|(index, account_id)| QueuedPlayer {
            account_id: *account_id,
            public_player_id: Uuid::new_v4(),
            enqueued_at: queued_at + Duration::seconds(index as i64),
        })
        .collect::<Vec<_>>();
    CreatePreparingMatch {
        match_id,
        world_name: format!("match-{}", match_id.simple()),
        seed: (match_id.as_u128() as u64) & i64::MAX as u64,
        versions: MatchVersions {
            generation: "generation-v1".to_owned(),
            gameplay: "gameplay-v1".to_owned(),
            config: "config-v1".to_owned(),
        },
        created_at,
        roster: FrozenRoster::try_from(players).expect("测试名单必须恰好 10 人"),
    }
}

fn test_database_url() -> String {
    env::var("TEST_DATABASE_URL")
        .or_else(|_| env::var("DATABASE_URL"))
        .expect("启用 db-tests 时必须设置 TEST_DATABASE_URL 或 DATABASE_URL")
}
