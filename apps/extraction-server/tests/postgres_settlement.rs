#![cfg(feature = "db-tests")]

use std::env;

use extraction_server::{
    matchmaking::{
        CommitSettlement, CreatePreparingMatch, ExtractionQualification, FrozenRoster,
        MatchVersions, QueuedPlayer, SettlementResources, MATCH_SIZE,
    },
    persistence::{migrate_database, PgRepository},
    ports::{
        AuthRepository, MatchRepository, SettlementRepository, SettlementRepositoryError,
        TransitionOutcome,
    },
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

static SETTLEMENT_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[actix_web::test]
async fn settlement_commit_is_atomic_and_same_digest_retry_is_idempotent() {
    let _guard = SETTLEMENT_TEST_LOCK.lock().await;
    let repository = test_repository().await;
    let now = OffsetDateTime::now_utc();
    let fixture = extraction_fixture(&repository, now - Duration::minutes(9)).await;
    let resources = SettlementResources::new(7, 2, 1);
    let qualification = make_qualification(&fixture, now - Duration::seconds(30), resources);
    let pending = repository
        .mark_settlement_pending(qualification.clone())
        .await
        .expect("撤离资格应先持久化为待结算");
    assert_eq!(pending.value().stats, qualification.stats);
    let mut conflicting_stats = qualification.clone();
    conflicting_stats.stats.picked_up.dirt += 1;
    assert_eq!(
        repository.mark_settlement_pending(conflicting_stats).await,
        Err(SettlementRepositoryError::Conflict),
        "同一撤离资格不能用不同局内统计覆盖"
    );

    let first_command = CommitSettlement {
        settlement_id: Uuid::new_v4(),
        qualification: qualification.clone(),
    };
    let second_command = CommitSettlement {
        settlement_id: Uuid::new_v4(),
        qualification: qualification.clone(),
    };
    let first = {
        let repository = repository.clone();
        actix_web::rt::spawn(async move { repository.commit_settlement(first_command).await })
    };
    let second = {
        let repository = repository.clone();
        actix_web::rt::spawn(async move { repository.commit_settlement(second_command).await })
    };
    let outcomes = actix_web::rt::time::timeout(std::time::Duration::from_secs(5), async {
        [
            first.await.expect("首个结算任务不应崩溃").unwrap(),
            second.await.expect("重试结算任务不应崩溃").unwrap(),
        ]
    })
    .await
    .expect("同一账号结算锁序不应死锁");

    assert_eq!(
        outcomes
            .iter()
            .filter(|outcome| outcome.was_applied())
            .count(),
        1
    );
    assert_eq!(outcomes[0].value(), outcomes[1].value());
    assert_eq!(outcomes[0].value().resources, resources);
    assert_eq!(outcomes[0].value().total_value, 127);

    let recovery = repository
        .commit_settlement(CommitSettlement {
            settlement_id: Uuid::new_v4(),
            qualification: qualification.clone(),
        })
        .await
        .expect("提交响应丢失后的新进程可按幂等键恢复");
    assert!(matches!(recovery, TransitionOutcome::AlreadyApplied(_)));
    assert_eq!(recovery.value(), outcomes[0].value());

    let warehouse = repository.warehouse(fixture.account_id).await.unwrap();
    assert_eq!(
        (warehouse.dirt, warehouse.gold, warehouse.diamond),
        (7, 2, 1)
    );
    assert_eq!(warehouse.stats.total_resources_extracted, 10);
    assert_eq!(warehouse.stats.total_extraction_value, 127);
    assert_eq!(warehouse.stats.successful_extractions, 1);
    assert_eq!(warehouse.stats.highest_single_match_value, 127);
    assert_eq!(
        count_rows(&repository, "extraction_settlements", fixture.account_id).await,
        1
    );
    assert_eq!(
        count_rows(&repository, "asset_ledger", fixture.account_id).await,
        3
    );

    let exact_result = repository
        .find_match_result(fixture.match_id, fixture.account_id)
        .await
        .unwrap()
        .expect("本人比赛结果应存在");
    assert_eq!(exact_result.stats, qualification.stats);
    assert_eq!(exact_result.settlement.as_ref(), Some(outcomes[0].value()));
    let latest_result = repository
        .find_latest_match_result(fixture.account_id)
        .await
        .unwrap()
        .expect("最新比赛结果应来自同一快照");
    assert_eq!(latest_result, exact_result);
}

#[actix_web::test]
async fn different_digest_retry_conflicts_without_duplicate_assets() {
    let _guard = SETTLEMENT_TEST_LOCK.lock().await;
    let repository = test_repository().await;
    let now = OffsetDateTime::now_utc();
    let fixture = extraction_fixture(&repository, now - Duration::minutes(9)).await;
    let qualification = make_qualification(
        &fixture,
        now - Duration::seconds(30),
        SettlementResources::new(2, 1, 0),
    );
    repository
        .mark_settlement_pending(qualification.clone())
        .await
        .unwrap();
    repository
        .commit_settlement(CommitSettlement {
            settlement_id: Uuid::new_v4(),
            qualification: qualification.clone(),
        })
        .await
        .unwrap();

    let shifted_time = make_qualification(
        &fixture,
        qualification.qualified_at + Duration::seconds(1),
        qualification.resources,
    );
    assert_eq!(
        repository
            .commit_settlement(CommitSettlement {
                settlement_id: Uuid::new_v4(),
                qualification: shifted_time,
            })
            .await,
        Err(SettlementRepositoryError::Conflict)
    );
    let conflicting = make_qualification(
        &fixture,
        qualification.qualified_at,
        SettlementResources::new(3, 1, 0),
    );
    assert_eq!(
        repository
            .commit_settlement(CommitSettlement {
                settlement_id: Uuid::new_v4(),
                qualification: conflicting,
            })
            .await,
        Err(SettlementRepositoryError::Conflict)
    );
    assert_eq!(
        count_rows(&repository, "extraction_settlements", fixture.account_id).await,
        1
    );
    assert_eq!(
        count_rows(&repository, "asset_ledger", fixture.account_id).await,
        2
    );
    let warehouse = repository.warehouse(fixture.account_id).await.unwrap();
    assert_eq!(
        (warehouse.dirt, warehouse.gold, warehouse.diamond),
        (2, 1, 0)
    );
}

#[actix_web::test]
async fn empty_inventory_commits_one_zero_value_settlement() {
    let _guard = SETTLEMENT_TEST_LOCK.lock().await;
    let repository = test_repository().await;
    let now = OffsetDateTime::now_utc();
    let fixture = extraction_fixture(&repository, now - Duration::minutes(9)).await;
    let qualification = make_qualification(
        &fixture,
        now - Duration::seconds(30),
        SettlementResources::default(),
    );
    repository
        .mark_settlement_pending(qualification.clone())
        .await
        .unwrap();
    let settled = repository
        .commit_settlement(CommitSettlement {
            settlement_id: Uuid::new_v4(),
            qualification,
        })
        .await
        .unwrap()
        .into_value();

    assert_eq!(settled.total_value, 0);
    assert!(settled.resources.is_empty());
    assert_eq!(
        count_rows(&repository, "extraction_settlements", fixture.account_id).await,
        1
    );
    assert_eq!(
        count_rows(&repository, "asset_ledger", fixture.account_id).await,
        0
    );
    assert_eq!(
        count_rows(&repository, "warehouse_balances", fixture.account_id).await,
        0
    );
    let warehouse = repository.warehouse(fixture.account_id).await.unwrap();
    assert_eq!(warehouse.stats.successful_extractions, 1);
    assert_eq!(warehouse.stats.total_resources_extracted, 0);
    assert_eq!(warehouse.stats.total_extraction_value, 0);
}

#[actix_web::test]
async fn settlement_read_rejects_ledger_detail_drift() {
    let _guard = SETTLEMENT_TEST_LOCK.lock().await;
    let repository = test_repository().await;
    let now = OffsetDateTime::now_utc();
    let fixture = extraction_fixture(&repository, now - Duration::minutes(9)).await;
    let qualification = make_qualification(
        &fixture,
        now - Duration::seconds(30),
        SettlementResources::new(2, 0, 0),
    );
    repository
        .mark_settlement_pending(qualification.clone())
        .await
        .unwrap();
    let settlement = repository
        .commit_settlement(CommitSettlement {
            settlement_id: Uuid::new_v4(),
            qualification,
        })
        .await
        .unwrap()
        .into_value();
    sqlx::query(
        "UPDATE asset_ledger SET delta = delta + 1 \
         WHERE settlement_id = $1 AND item_key = 'dirt'",
    )
    .bind(settlement.settlement_id)
    .execute(repository.pool())
    .await
    .unwrap();

    assert_eq!(
        repository
            .find_settlement(fixture.match_id, fixture.account_id)
            .await,
        Err(SettlementRepositoryError::Invariant)
    );
}

#[actix_web::test]
async fn closed_grace_window_performs_no_asset_writes() {
    let _guard = SETTLEMENT_TEST_LOCK.lock().await;
    let repository = test_repository().await;
    let now = OffsetDateTime::now_utc();
    let fixture = extraction_fixture(&repository, now - Duration::minutes(9)).await;
    let qualification = make_qualification(
        &fixture,
        now - Duration::seconds(30),
        SettlementResources::new(1, 0, 0),
    );
    repository
        .mark_settlement_pending(qualification.clone())
        .await
        .unwrap();
    let closed_hard_deadline = now - Duration::seconds(1);
    let closed_grace_deadline = now - Duration::milliseconds(500);
    sqlx::query(
        "UPDATE matches SET hard_deadline = $2, settlement_grace_deadline = $3 WHERE id = $1",
    )
    .bind(fixture.match_id)
    .bind(closed_hard_deadline)
    .bind(closed_grace_deadline)
    .execute(repository.pool())
    .await
    .unwrap();

    let late_qualification = ExtractionQualification::new(
        fixture.match_id,
        fixture.second_account_id,
        closed_hard_deadline - Duration::seconds(1),
        SettlementResources::new(1, 0, 0),
        qualification_stats(),
        "balance-v1".to_owned(),
    )
    .unwrap();
    assert_eq!(
        repository.mark_settlement_pending(late_qualification).await,
        Err(SettlementRepositoryError::WindowClosed)
    );

    assert_eq!(
        repository
            .commit_settlement(CommitSettlement {
                settlement_id: Uuid::new_v4(),
                qualification,
            })
            .await,
        Err(SettlementRepositoryError::WindowClosed)
    );
    assert_eq!(
        count_rows(&repository, "extraction_settlements", fixture.account_id).await,
        0
    );
    assert_eq!(
        count_rows(&repository, "asset_ledger", fixture.account_id).await,
        0
    );
    assert_eq!(
        count_rows(&repository, "warehouse_balances", fixture.account_id).await,
        0
    );
    assert_eq!(
        repository
            .abort_pending_settlement(fixture.match_id, fixture.account_id, now)
            .await,
        Err(SettlementRepositoryError::WindowClosed)
    );
    let projected = repository
        .find_match_result(fixture.match_id, fixture.account_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        projected.participant_state,
        extraction_server::matchmaking::ParticipantState::Aborted
    );
    let persisted = repository
        .find_match(fixture.match_id)
        .await
        .unwrap()
        .unwrap();
    let participant = persisted
        .participants
        .iter()
        .find(|participant| participant.account_id == fixture.account_id)
        .unwrap();
    assert_eq!(
        participant.state,
        extraction_server::matchmaking::ParticipantState::SettlementPending
    );
}

#[actix_web::test]
async fn warehouse_overflow_rolls_back_entire_settlement() {
    let _guard = SETTLEMENT_TEST_LOCK.lock().await;
    let repository = test_repository().await;
    let now = OffsetDateTime::now_utc();
    let fixture = extraction_fixture(&repository, now - Duration::minutes(9)).await;
    let qualification = make_qualification(
        &fixture,
        now - Duration::seconds(30),
        SettlementResources::new(1, 0, 0),
    );
    repository
        .mark_settlement_pending(qualification.clone())
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO warehouse_balances (account_id, item_key, quantity, updated_at) \
         VALUES ($1, 'dirt', $2, $3)",
    )
    .bind(fixture.account_id)
    .bind(i64::MAX)
    .bind(now)
    .execute(repository.pool())
    .await
    .unwrap();

    assert_eq!(
        repository
            .commit_settlement(CommitSettlement {
                settlement_id: Uuid::new_v4(),
                qualification,
            })
            .await,
        Err(SettlementRepositoryError::Overflow)
    );
    assert_eq!(
        count_rows(&repository, "extraction_settlements", fixture.account_id).await,
        0
    );
    assert_eq!(
        count_rows(&repository, "asset_ledger", fixture.account_id).await,
        0
    );
    let balance = sqlx::query_scalar::<_, i64>(
        "SELECT quantity FROM warehouse_balances WHERE account_id = $1 AND item_key = 'dirt'",
    )
    .bind(fixture.account_id)
    .fetch_one(repository.pool())
    .await
    .unwrap();
    assert_eq!(balance, i64::MAX);
}

#[actix_web::test]
async fn cumulative_ledger_and_value_overflow_roll_back_entire_settlement() {
    let _guard = SETTLEMENT_TEST_LOCK.lock().await;
    let repository = test_repository().await;
    let now = OffsetDateTime::now_utc();
    let fixture = extraction_fixture(&repository, now - Duration::minutes(9)).await;
    let qualification = make_qualification(
        &fixture,
        now - Duration::seconds(30),
        SettlementResources::new(1, 0, 0),
    );
    repository
        .mark_settlement_pending(qualification.clone())
        .await
        .unwrap();
    insert_maximum_historical_settlement(&repository, fixture.account_id, now).await;

    assert_eq!(
        repository
            .commit_settlement(CommitSettlement {
                settlement_id: Uuid::new_v4(),
                qualification,
            })
            .await,
        Err(SettlementRepositoryError::Overflow)
    );
    let current_settlements = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM extraction_settlements WHERE match_id = $1 AND account_id = $2",
    )
    .bind(fixture.match_id)
    .bind(fixture.account_id)
    .fetch_one(repository.pool())
    .await
    .unwrap();
    assert_eq!(current_settlements, 0);
    assert_eq!(
        count_rows(&repository, "extraction_settlements", fixture.account_id).await,
        1
    );
    assert_eq!(
        count_rows(&repository, "asset_ledger", fixture.account_id).await,
        1
    );
}

struct ExtractionFixture {
    match_id: Uuid,
    account_id: Uuid,
    second_account_id: Uuid,
}

async fn extraction_fixture(
    repository: &PgRepository,
    started_at: OffsetDateTime,
) -> ExtractionFixture {
    let accounts = insert_accounts(repository, MATCH_SIZE).await;
    let command = preparing_command(&accounts, started_at - Duration::seconds(1));
    let match_id = command.match_id;
    repository.create_preparing(command).await.unwrap();
    let active = repository
        .activate(match_id, started_at)
        .await
        .unwrap()
        .into_value();
    let extraction_open_at = active.record.extraction_open_at.unwrap();
    repository
        .open_extraction(match_id, extraction_open_at)
        .await
        .unwrap();
    ExtractionFixture {
        match_id,
        account_id: accounts[0],
        second_account_id: accounts[1],
    }
}

fn make_qualification(
    fixture: &ExtractionFixture,
    qualified_at: OffsetDateTime,
    resources: SettlementResources,
) -> ExtractionQualification {
    ExtractionQualification::new(
        fixture.match_id,
        fixture.account_id,
        qualified_at,
        resources,
        qualification_stats(),
        "balance-v1".to_owned(),
    )
    .unwrap()
}

fn qualification_stats() -> extraction_server::matchmaking::ParticipantMatchStats {
    use extraction_server::matchmaking::{ParticipantMatchStats, ParticipantResourceCounts};

    ParticipantMatchStats {
        mined: ParticipantResourceCounts {
            dirt: 4,
            gold: 2,
            diamond: 1,
        },
        picked_up: ParticipantResourceCounts {
            dirt: 3,
            gold: 1,
            diamond: 0,
        },
        lost: ParticipantResourceCounts::default(),
    }
}

async fn count_rows(repository: &PgRepository, table: &str, account_id: Uuid) -> i64 {
    let query = format!("SELECT COUNT(*) FROM {table} WHERE account_id = $1");
    sqlx::query_scalar(&query)
        .bind(account_id)
        .fetch_one(repository.pool())
        .await
        .unwrap()
}

async fn insert_maximum_historical_settlement(
    repository: &PgRepository,
    account_id: Uuid,
    now: OffsetDateTime,
) {
    let match_id = Uuid::new_v4();
    let settlement_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO matches (id, state, world_name, seed, generation_version, gameplay_version, \
         config_version, created_at, finished_at) \
         VALUES ($1, 'finished', $2, 0, 'generation-v1', 'pvp-mvp-v1', \
         'balance-v1', $3, $3)",
    )
    .bind(match_id)
    .bind(format!("historical-{}", match_id.simple()))
    .bind(now - Duration::days(1))
    .execute(repository.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO match_participants (match_id, account_id, public_player_id, seat_id, state, \
         enqueued_at, extracted_at, settlement_qualified_at) \
         VALUES ($1, $2, $3, 0, 'extracted', $4, $4, $4)",
    )
    .bind(match_id)
    .bind(account_id)
    .bind(Uuid::new_v4())
    .bind(now - Duration::days(1))
    .execute(repository.pool())
    .await
    .unwrap();
    let resources = SettlementResources::new(i64::MAX as u64, 0, 0);
    sqlx::query(
        "INSERT INTO extraction_settlements (id, match_id, account_id, idempotency_key, \
         inventory_digest, config_version, total_value, committed_at) \
         VALUES ($1, $2, $3, $4, $5, 'balance-v1', $6, $7)",
    )
    .bind(settlement_id)
    .bind(match_id)
    .bind(account_id)
    .bind(format!("extract:v1:{match_id}:{account_id}"))
    .bind(resources.digest().as_slice())
    .bind(i64::MAX)
    .bind(now - Duration::days(1))
    .execute(repository.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO settlement_items (settlement_id, item_key, quantity) \
         VALUES ($1, 'dirt', $2)",
    )
    .bind(settlement_id)
    .bind(i64::MAX)
    .execute(repository.pool())
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO asset_ledger (id, account_id, settlement_id, item_key, delta, created_at) \
         VALUES ($1, $2, $3, 'dirt', $4, $5)",
    )
    .bind(Uuid::new_v4())
    .bind(account_id)
    .bind(settlement_id)
    .bind(i64::MAX)
    .bind(now - Duration::days(1))
    .execute(repository.pool())
    .await
    .unwrap();
}

async fn test_repository() -> PgRepository {
    let database_url = test_database_url();
    migrate_database(&database_url).await.unwrap();
    PgRepository::connect(&database_url).await.unwrap()
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
            .unwrap();
    }
    accounts
}

fn preparing_command(accounts: &[Uuid], created_at: OffsetDateTime) -> CreatePreparingMatch {
    let match_id = Uuid::new_v4();
    let players = accounts
        .iter()
        .enumerate()
        .map(|(index, account_id)| QueuedPlayer {
            account_id: *account_id,
            public_player_id: Uuid::new_v4(),
            enqueued_at: created_at - Duration::minutes(1) + Duration::seconds(index as i64),
        })
        .collect::<Vec<_>>();
    CreatePreparingMatch {
        match_id,
        world_name: format!("match-{}", match_id.simple()),
        seed: (match_id.as_u128() as u64) & i64::MAX as u64,
        versions: MatchVersions {
            generation: "generation-v1".to_owned(),
            gameplay: "pvp-mvp-v1".to_owned(),
            config: "balance-v1".to_owned(),
        },
        created_at,
        roster: FrozenRoster::try_from(players).unwrap(),
    }
}

fn test_database_url() -> String {
    env::var("TEST_DATABASE_URL")
        .or_else(|_| env::var("DATABASE_URL"))
        .expect("启用 db-tests 时必须设置 TEST_DATABASE_URL 或 DATABASE_URL")
}
