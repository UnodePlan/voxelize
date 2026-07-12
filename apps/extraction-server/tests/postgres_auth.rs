#![cfg(feature = "db-tests")]

use std::{env, sync::Arc, time::Duration as StdDuration};

use extraction_server::{
    persistence::{migrate_database, PgRepository},
    ports::{AuthRepository, AuthRepositoryError, LoginCommand, NewNonce},
};
use sqlx::postgres::PgPoolOptions;
use time::{Duration, OffsetDateTime};
use tokio::sync::Barrier;
use uuid::Uuid;

const DOMAIN: &str = "127.0.0.1:5173";
const URI: &str = "http://127.0.0.1:5173";

#[actix_web::test]
async fn concurrent_nonce_consumption_allows_exactly_one_login() {
    let repository = test_repository().await;
    let now = OffsetDateTime::now_utc();
    let nonce_hash = unique_hash();
    let address = unique_address();
    insert_nonce(&repository, nonce_hash, now).await;

    let mut attempts = Vec::new();
    let mut token_hashes = Vec::new();
    for _ in 0..16 {
        let token_hash = unique_hash();
        token_hashes.push(token_hash);
        let command = login_command(nonce_hash, address, token_hash, now);
        let repository = repository.clone();
        attempts.push(actix_web::rt::spawn(async move {
            repository.complete_login(command).await
        }));
    }

    let mut successful_logins = 0;
    let mut rejected_replays = 0;
    for attempt in attempts {
        match attempt.await.expect("并发登录任务不应崩溃") {
            Ok(_) => successful_logins += 1,
            Err(AuthRepositoryError::NonceInvalid) => rejected_replays += 1,
            Err(error) => panic!("并发登录出现非预期仓储错误: {error:?}"),
        }
    }

    assert_eq!(successful_logins, 1);
    assert_eq!(rejected_replays, 15);
    assert!(repository
        .find_nonce(nonce_hash)
        .await
        .expect("应能读取已消费 nonce")
        .expect("nonce 应存在")
        .consumed_at
        .is_some());

    let mut active_sessions = 0;
    for token_hash in token_hashes {
        if repository
            .find_active_session(token_hash, now + Duration::seconds(1))
            .await
            .expect("应能查询并发登录产生的会话")
            .is_some()
        {
            active_sessions += 1;
        }
    }
    assert_eq!(active_sessions, 1);
}

#[actix_web::test]
async fn second_login_for_same_wallet_revokes_the_old_session() {
    let repository = test_repository().await;
    let first_login_at = OffsetDateTime::now_utc();
    let second_login_at = first_login_at + Duration::seconds(1);
    let address = unique_address();
    let first_nonce_hash = unique_hash();
    let second_nonce_hash = unique_hash();
    let first_token_hash = unique_hash();
    let second_token_hash = unique_hash();

    insert_nonce(&repository, first_nonce_hash, first_login_at).await;
    let first_login = repository
        .complete_login(login_command(
            first_nonce_hash,
            address,
            first_token_hash,
            first_login_at,
        ))
        .await
        .expect("首次登录应成功");
    assert!(first_login.revoked_session_ids.is_empty());
    let first_session = first_login.session;

    insert_nonce(&repository, second_nonce_hash, second_login_at).await;
    let second_login = repository
        .complete_login(login_command(
            second_nonce_hash,
            address,
            second_token_hash,
            second_login_at,
        ))
        .await
        .expect("同钱包二次登录应成功");
    assert_eq!(
        second_login.revoked_session_ids,
        vec![first_session.session_id]
    );
    let second_session = second_login.session;

    assert_eq!(second_session.account_id, first_session.account_id);
    assert_ne!(second_session.session_id, first_session.session_id);
    assert!(repository
        .find_active_session(first_token_hash, second_login_at + Duration::seconds(1))
        .await
        .expect("应能查询旧会话")
        .is_none());

    let active_session = repository
        .find_active_session(second_token_hash, second_login_at + Duration::seconds(1))
        .await
        .expect("应能查询新会话")
        .expect("新会话应保持有效");
    assert_eq!(active_session.session_id, second_session.session_id);
    assert_eq!(active_session.account_id, first_session.account_id);
    assert_eq!(active_session.address, address);
}

#[actix_web::test]
async fn concurrent_logins_for_same_wallet_are_serialized_and_report_replacement() {
    let repository = test_repository().await;
    let first_login_at = OffsetDateTime::now_utc();
    let concurrent_login_at = first_login_at + Duration::seconds(1);
    let address = unique_address();
    let initial_nonce_hash = unique_hash();
    let initial_token_hash = unique_hash();
    insert_nonce(&repository, initial_nonce_hash, first_login_at).await;
    let initial = repository
        .complete_login(login_command(
            initial_nonce_hash,
            address,
            initial_token_hash,
            first_login_at,
        ))
        .await
        .expect("初始登录应成功");

    let first_nonce_hash = unique_hash();
    let second_nonce_hash = unique_hash();
    let first_token_hash = unique_hash();
    let second_token_hash = unique_hash();
    insert_nonce(&repository, first_nonce_hash, concurrent_login_at).await;
    insert_nonce(&repository, second_nonce_hash, concurrent_login_at).await;

    let barrier = Arc::new(Barrier::new(3));
    let first_attempt = {
        let repository = repository.clone();
        let barrier = barrier.clone();
        let command = login_command(
            first_nonce_hash,
            address,
            first_token_hash,
            concurrent_login_at,
        );
        actix_web::rt::spawn(async move {
            barrier.wait().await;
            repository.complete_login(command).await
        })
    };
    let second_attempt = {
        let repository = repository.clone();
        let barrier = barrier.clone();
        let command = login_command(
            second_nonce_hash,
            address,
            second_token_hash,
            concurrent_login_at,
        );
        actix_web::rt::spawn(async move {
            barrier.wait().await;
            repository.complete_login(command).await
        })
    };
    barrier.wait().await;

    let first = first_attempt
        .await
        .expect("第一个并发任务不应崩溃")
        .expect("第一个并发登录不应因唯一约束返回 503");
    let second = second_attempt
        .await
        .expect("第二个并发任务不应崩溃")
        .expect("第二个并发登录不应因唯一约束返回 503");
    assert_eq!(first.session.account_id, initial.session.account_id);
    assert_eq!(second.session.account_id, initial.session.account_id);
    assert!(
        first
            .revoked_session_ids
            .contains(&initial.session.session_id)
            || second
                .revoked_session_ids
                .contains(&initial.session.session_id),
        "首个串行执行的并发登录应报告替换初始会话"
    );

    let first_active = repository
        .find_active_session(first_token_hash, concurrent_login_at + Duration::seconds(1))
        .await
        .expect("应能查询第一个并发会话");
    let second_active = repository
        .find_active_session(
            second_token_hash,
            concurrent_login_at + Duration::seconds(1),
        )
        .await
        .expect("应能查询第二个并发会话");
    match (first_active, second_active) {
        (Some(active), None) => {
            assert_eq!(active.session_id, first.session.session_id);
            assert!(first
                .revoked_session_ids
                .contains(&second.session.session_id));
        }
        (None, Some(active)) => {
            assert_eq!(active.session_id, second.session.session_id);
            assert!(second
                .revoked_session_ids
                .contains(&first.session.session_id));
        }
        _ => panic!("同钱包并发登录完成后必须恰好保留一个活跃会话"),
    }
    assert!(repository
        .find_active_session(
            initial_token_hash,
            concurrent_login_at + Duration::seconds(1),
        )
        .await
        .expect("应能查询初始会话")
        .is_none());
}

#[actix_web::test]
async fn an_earlier_captured_login_time_can_still_replace_a_newer_session() {
    let repository = test_repository().await;
    let base = OffsetDateTime::now_utc();
    let newer_time = base + Duration::seconds(2);
    let earlier_time = base + Duration::seconds(1);
    let address = unique_address();
    let newer_nonce = unique_hash();
    let earlier_nonce = unique_hash();
    let newer_token = unique_hash();
    let earlier_token = unique_hash();

    insert_nonce(&repository, newer_nonce, newer_time).await;
    insert_nonce(&repository, earlier_nonce, earlier_time).await;
    let newer_session = repository
        .complete_login(login_command(newer_nonce, address, newer_token, newer_time))
        .await
        .expect("较新时间戳的会话应创建成功")
        .session;
    let replacement = repository
        .complete_login(login_command(
            earlier_nonce,
            address,
            earlier_token,
            earlier_time,
        ))
        .await
        .expect("较早捕获时间的请求仍应按账号锁顺序完成替换");

    assert_eq!(
        replacement.revoked_session_ids,
        vec![newer_session.session_id]
    );
    assert!(repository
        .find_active_session(newer_token, newer_time + Duration::seconds(1))
        .await
        .expect("应能查询被替换会话")
        .is_none());
    assert!(repository
        .find_active_session(earlier_token, newer_time + Duration::seconds(1))
        .await
        .expect("应能查询替换会话")
        .is_some());
}

#[actix_web::test]
async fn session_activity_never_moves_backward_and_inspection_is_read_only() {
    let repository = test_repository().await;
    let login_at = OffsetDateTime::now_utc();
    let nonce_hash = unique_hash();
    let token_hash = unique_hash();
    insert_nonce(&repository, nonce_hash, login_at).await;
    let login = repository
        .complete_login(login_command(
            nonce_hash,
            unique_address(),
            token_hash,
            login_at,
        ))
        .await
        .expect("测试登录应成功");

    let active = repository
        .find_active_session(token_hash, login_at + Duration::hours(2))
        .await
        .expect("活跃认证应成功")
        .expect("会话应保持有效");
    let expected_idle = login_at + Duration::hours(26);
    assert_eq!(active.idle_expires_at, expected_idle);
    assert_eq!(
        session_timestamps(&repository, login.session.session_id).await,
        (login_at + Duration::hours(2), expected_idle, None)
    );

    let rolled_back = repository
        .find_active_session(token_hash, login_at + Duration::hours(1))
        .await
        .expect("回退时钟下的活跃认证不应失败")
        .expect("回退时钟不应让有效会话消失");
    assert_eq!(rolled_back.idle_expires_at, expected_idle);
    assert_eq!(
        session_timestamps(&repository, login.session.session_id).await,
        (login_at + Duration::hours(2), expected_idle, None)
    );

    let inspected = repository
        .inspect_active_session(token_hash, login_at + Duration::hours(3))
        .await
        .expect("只读会话检查应成功")
        .expect("会话应保持有效");
    assert_eq!(inspected.idle_expires_at, expected_idle);
    assert_eq!(
        session_timestamps(&repository, login.session.session_id).await,
        (login_at + Duration::hours(2), expected_idle, None)
    );
    assert!(repository
        .inspect_active_session(token_hash, expected_idle)
        .await
        .expect("过期边界检查应成功")
        .is_none());

    assert_eq!(
        repository
            .revoke_session(token_hash, login_at - Duration::hours(1))
            .await
            .expect("回退时钟下撤销应成功"),
        Some(login.session.session_id)
    );
    assert!(session_timestamps(&repository, login.session.session_id)
        .await
        .2
        .is_some_and(|revoked_at| revoked_at >= login_at));
}

#[actix_web::test]
async fn nonce_consumption_never_precedes_nonce_creation() {
    let repository = test_repository().await;
    let created_at = OffsetDateTime::now_utc();
    let captured_at = created_at - Duration::hours(1);
    let nonce_hash = unique_hash();
    insert_nonce_with_times(
        &repository,
        nonce_hash,
        created_at,
        created_at + Duration::minutes(5),
    )
    .await;

    let mut command = login_command(nonce_hash, unique_address(), unique_hash(), captured_at);
    command.authentication_expires_at = created_at + Duration::minutes(5);
    repository
        .complete_login(command)
        .await
        .expect("回退时钟下 nonce 消费应满足数据库时间约束");
    assert!(repository
        .find_nonce(nonce_hash)
        .await
        .expect("应能读取 nonce")
        .expect("nonce 应存在")
        .consumed_at
        .is_some_and(|consumed_at| consumed_at >= created_at));
}

#[actix_web::test]
async fn nonce_expiring_while_the_atomic_update_waits_is_rejected() {
    let repository = test_repository().await;
    let now = OffsetDateTime::now_utc();
    let nonce_hash = unique_hash();
    let token_hash = unique_hash();
    insert_nonce_with_times(&repository, nonce_hash, now, now + Duration::seconds(1)).await;

    let mut blocker = repository.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM auth_nonces WHERE nonce_hash = $1 FOR UPDATE")
        .bind(nonce_hash.as_slice())
        .fetch_one(&mut *blocker)
        .await
        .expect("应能锁定测试 nonce");
    let attempt = {
        let repository = repository.clone();
        actix_web::rt::spawn(async move {
            repository
                .complete_login(login_command(nonce_hash, unique_address(), token_hash, now))
                .await
        })
    };
    actix_web::rt::time::sleep(StdDuration::from_millis(1_200)).await;
    blocker.commit().await.unwrap();

    assert_eq!(
        attempt.await.expect("等待中的登录任务不应崩溃"),
        Err(AuthRepositoryError::NonceInvalid)
    );
}

#[actix_web::test]
async fn authentication_expiring_while_the_account_lock_waits_is_rejected() {
    let repository = test_repository().await;
    let now = OffsetDateTime::now_utc();
    let address = unique_address();
    let initial_nonce = unique_hash();
    let initial_token = unique_hash();
    insert_nonce(&repository, initial_nonce, now).await;
    let initial_session = repository
        .complete_login(login_command(initial_nonce, address, initial_token, now))
        .await
        .expect("初始登录应成功")
        .session;

    let nonce_hash = unique_hash();
    let token_hash = unique_hash();
    insert_nonce(&repository, nonce_hash, now).await;
    let mut blocker = repository.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM accounts WHERE id = $1 FOR UPDATE")
        .bind(initial_session.account_id)
        .fetch_one(&mut *blocker)
        .await
        .expect("应能锁定测试账号");
    let attempt = {
        let repository = repository.clone();
        let mut command = login_command(nonce_hash, address, token_hash, now);
        command.authentication_expires_at = now + Duration::seconds(1);
        actix_web::rt::spawn(async move { repository.complete_login(command).await })
    };
    actix_web::rt::time::sleep(StdDuration::from_millis(1_200)).await;
    blocker.commit().await.unwrap();

    assert_eq!(
        attempt.await.expect("等待中的登录任务不应崩溃"),
        Err(AuthRepositoryError::AuthenticationExpired)
    );
    assert!(repository
        .find_nonce(nonce_hash)
        .await
        .expect("应能读取回滚后的 nonce")
        .expect("nonce 应存在")
        .consumed_at
        .is_none());
    assert!(repository
        .find_active_session(initial_token, now + Duration::seconds(2))
        .await
        .expect("应能查询原会话")
        .is_some());
    assert!(repository
        .find_active_session(token_hash, now + Duration::seconds(2))
        .await
        .expect("应能查询被拒绝的新会话")
        .is_none());
}

#[actix_web::test]
async fn authentication_expiring_while_the_old_session_lock_waits_is_rejected() {
    let repository = test_repository().await;
    let now = OffsetDateTime::now_utc();
    let address = unique_address();
    let initial_nonce = unique_hash();
    let initial_token = unique_hash();
    insert_nonce(&repository, initial_nonce, now).await;
    let initial_session = repository
        .complete_login(login_command(initial_nonce, address, initial_token, now))
        .await
        .expect("初始登录应成功")
        .session;

    let nonce_hash = unique_hash();
    let token_hash = unique_hash();
    insert_nonce(&repository, nonce_hash, now).await;
    let mut blocker = repository.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM auth_sessions WHERE id = $1 FOR UPDATE")
        .bind(initial_session.session_id)
        .fetch_one(&mut *blocker)
        .await
        .expect("应能锁定旧会话");
    let attempt = {
        let repository = repository.clone();
        let mut command = login_command(nonce_hash, address, token_hash, now);
        command.authentication_expires_at = now + Duration::seconds(1);
        actix_web::rt::spawn(async move { repository.complete_login(command).await })
    };
    actix_web::rt::time::sleep(StdDuration::from_millis(1_200)).await;
    blocker.commit().await.unwrap();

    assert_eq!(
        attempt.await.expect("等待中的登录任务不应崩溃"),
        Err(AuthRepositoryError::AuthenticationExpired)
    );
    assert!(repository
        .find_nonce(nonce_hash)
        .await
        .expect("应能读取回滚后的 nonce")
        .expect("nonce 应存在")
        .consumed_at
        .is_none());
    assert!(repository
        .find_active_session(initial_token, now + Duration::seconds(2))
        .await
        .expect("应能查询原会话")
        .is_some());
    assert!(repository
        .find_active_session(token_hash, now + Duration::seconds(2))
        .await
        .expect("应能查询被拒绝的新会话")
        .is_none());
}

#[actix_web::test]
async fn session_expiring_while_the_touch_waits_is_rejected() {
    let repository = test_repository().await;
    let now = OffsetDateTime::now_utc();
    let nonce_hash = unique_hash();
    let token_hash = unique_hash();
    insert_nonce(&repository, nonce_hash, now).await;
    let mut command = login_command(nonce_hash, unique_address(), token_hash, now);
    command.session_idle_expires_at = now + Duration::seconds(1);
    let session = repository
        .complete_login(command)
        .await
        .expect("短 idle 会话应创建成功")
        .session;

    let mut blocker = repository.pool().begin().await.unwrap();
    sqlx::query("SELECT id FROM auth_sessions WHERE id = $1 FOR UPDATE")
        .bind(session.session_id)
        .fetch_one(&mut *blocker)
        .await
        .expect("应能锁定测试会话");
    let attempt = {
        let repository = repository.clone();
        actix_web::rt::spawn(async move { repository.find_active_session(token_hash, now).await })
    };
    actix_web::rt::time::sleep(StdDuration::from_millis(1_200)).await;
    blocker.commit().await.unwrap();

    assert!(attempt
        .await
        .expect("等待中的会话检查不应崩溃")
        .expect("会话检查不应返回仓储错误")
        .is_none());
}

#[actix_web::test]
async fn expired_nonce_pruning_is_bounded_and_preserves_live_rows() {
    let repository = test_repository().await;
    // 复用的测试库会保留旧数据；把样本放到当前最早记录之前，确保有界批次命中本用例。
    let earliest = earliest_nonce_expiration()
        .await
        .unwrap_or_else(OffsetDateTime::now_utc);
    let now = earliest - Duration::hours(1);
    let expired_hashes = [unique_hash(), unique_hash(), unique_hash()];
    let live_hash = unique_hash();
    for hash in expired_hashes {
        insert_nonce_with_times(
            &repository,
            hash,
            now - Duration::minutes(2),
            now - Duration::minutes(1),
        )
        .await;
    }
    insert_nonce_with_times(&repository, live_hash, now, now + Duration::minutes(5)).await;

    assert_eq!(
        repository
            .prune_expired_nonces(now, 2)
            .await
            .expect("首批 nonce 清理应成功"),
        2
    );
    let mut expired_remaining = 0;
    for hash in expired_hashes {
        if repository
            .find_nonce(hash)
            .await
            .expect("应能核对过期 nonce")
            .is_some()
        {
            expired_remaining += 1;
        }
    }
    assert_eq!(expired_remaining, 1);
    assert!(repository
        .find_nonce(live_hash)
        .await
        .expect("应能核对未过期 nonce")
        .is_some());
    assert_eq!(
        repository
            .prune_expired_nonces(now, 1)
            .await
            .expect("第二批 nonce 清理应成功"),
        1
    );
    assert!(repository
        .find_nonce(live_hash)
        .await
        .expect("清理后应能核对未过期 nonce")
        .is_some());
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

async fn earliest_nonce_expiration() -> Option<OffsetDateTime> {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&test_database_url())
        .await
        .expect("应能读取测试库时间窗");
    sqlx::query_scalar::<_, Option<OffsetDateTime>>("SELECT MIN(expires_at) FROM auth_nonces")
        .fetch_one(&pool)
        .await
        .expect("应能查询最早 nonce 过期时间")
}

async fn session_timestamps(
    repository: &PgRepository,
    session_id: Uuid,
) -> (OffsetDateTime, OffsetDateTime, Option<OffsetDateTime>) {
    sqlx::query_as::<_, (OffsetDateTime, OffsetDateTime, Option<OffsetDateTime>)>(
        "SELECT last_seen_at, idle_expires_at, revoked_at FROM auth_sessions WHERE id = $1",
    )
    .bind(session_id)
    .fetch_one(repository.pool())
    .await
    .expect("应能读取会话时间字段")
}

fn test_database_url() -> String {
    env::var("TEST_DATABASE_URL")
        .or_else(|_| env::var("DATABASE_URL"))
        .expect("启用 db-tests 时必须设置 TEST_DATABASE_URL 或 DATABASE_URL")
}

async fn insert_nonce(repository: &PgRepository, nonce_hash: [u8; 32], now: OffsetDateTime) {
    insert_nonce_with_times(repository, nonce_hash, now, now + Duration::minutes(5)).await;
}

async fn insert_nonce_with_times(
    repository: &PgRepository,
    nonce_hash: [u8; 32],
    created_at: OffsetDateTime,
    expires_at: OffsetDateTime,
) {
    repository
        .insert_nonce(NewNonce {
            id: Uuid::new_v4(),
            nonce_hash,
            domain: DOMAIN.to_owned(),
            uri: URI.to_owned(),
            created_at,
            expires_at,
        })
        .await
        .expect("测试 nonce 应写入成功");
}

fn login_command(
    nonce_hash: [u8; 32],
    address: [u8; 20],
    session_token_hash: [u8; 32],
    now: OffsetDateTime,
) -> LoginCommand {
    LoginCommand {
        nonce_hash,
        nonce_domain: DOMAIN.to_owned(),
        nonce_uri: URI.to_owned(),
        proposed_account_id: Uuid::new_v4(),
        session_id: Uuid::new_v4(),
        session_token_hash,
        chain_id: 1,
        address,
        now,
        authentication_expires_at: now + Duration::minutes(5),
        session_expires_at: now + Duration::days(7),
        session_idle_expires_at: now + Duration::days(1),
    }
}

fn unique_hash() -> [u8; 32] {
    let mut hash = [0_u8; 32];
    hash[..16].copy_from_slice(Uuid::new_v4().as_bytes());
    hash[16..].copy_from_slice(Uuid::new_v4().as_bytes());
    hash
}

fn unique_address() -> [u8; 20] {
    let mut address = [0_u8; 20];
    address[..16].copy_from_slice(Uuid::new_v4().as_bytes());
    address[16..].copy_from_slice(&Uuid::new_v4().as_bytes()[..4]);
    address
}
