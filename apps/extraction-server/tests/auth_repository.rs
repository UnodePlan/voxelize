mod support;

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::{Duration as StdDuration, SystemTime, UNIX_EPOCH},
};

use extraction_server::{
    auth::{AuthConfig, AuthError, AuthService},
    ports::{AuthRepository, Clock, NewNonce},
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use support::{service_at, siwe_message, AcceptVerifier, FixedRandom, MemoryRepository};

#[actix_web::test]
async fn memory_nonce_pruning_respects_limit_and_expiration() {
    let repository = MemoryRepository::new();
    let now = OffsetDateTime::from_unix_timestamp(1_752_372_000).unwrap();
    let first_expired = [1_u8; 32];
    let second_expired = [2_u8; 32];
    let live = [3_u8; 32];
    insert_nonce(
        repository.as_ref(),
        first_expired,
        now - Duration::minutes(1),
    )
    .await;
    insert_nonce(
        repository.as_ref(),
        second_expired,
        now - Duration::minutes(2),
    )
    .await;
    insert_nonce(repository.as_ref(), live, now + Duration::minutes(5)).await;

    assert_eq!(repository.prune_expired_nonces(now, 1).await.unwrap(), 1);
    assert_eq!(
        [first_expired, second_expired]
            .into_iter()
            .filter(|hash| { repository.state_has_nonce_for_test(*hash) })
            .count(),
        1
    );
    assert!(repository.find_nonce(live).await.unwrap().is_some());
    assert_eq!(repository.prune_expired_nonces(now, 1).await.unwrap(), 1);
    assert_eq!(repository.prune_expired_nonces(now, 0).await.unwrap(), 0);
    assert!(repository.find_nonce(live).await.unwrap().is_some());
}

#[actix_web::test]
async fn nonce_issuance_periodically_prunes_expired_rows() {
    let now = OffsetDateTime::from_unix_timestamp(1_752_372_000).unwrap();
    let (auth, repository) = service_at(now);
    let expired = [9_u8; 32];
    insert_nonce(repository.as_ref(), expired, now - Duration::seconds(1)).await;

    for _ in 0..63 {
        auth.issue_nonce().await.expect("测试 nonce 签发应成功");
    }
    assert!(repository.find_nonce(expired).await.unwrap().is_some());
    auth.issue_nonce().await.expect("第 64 次 nonce 签发应成功");
    actix_web::rt::time::timeout(StdDuration::from_millis(100), async {
        while repository.find_nonce(expired).await.unwrap().is_some() {
            actix_web::rt::task::yield_now().await;
        }
    })
    .await
    .expect("后台 nonce 清理应及时完成");
    assert!(repository.find_nonce(expired).await.unwrap().is_none());
}

#[actix_web::test]
async fn nonce_pruning_is_best_effort_and_single_flight() {
    let now = OffsetDateTime::from_unix_timestamp(1_752_372_000).unwrap();
    let (auth, repository) = service_at(now);
    repository.configure_nonce_pruning(StdDuration::from_millis(500), true);

    for _ in 0..63 {
        auth.issue_nonce().await.unwrap();
    }
    actix_web::rt::time::timeout(StdDuration::from_millis(100), auth.issue_nonce())
        .await
        .expect("后台清理不得阻塞第 64 次签发")
        .expect("后台清理失败不得改变签发结果");
    for _ in 0..64 {
        auth.issue_nonce().await.unwrap();
    }
    actix_web::rt::time::timeout(StdDuration::from_millis(100), async {
        while repository.nonce_prune_calls() == 0 {
            actix_web::rt::task::yield_now().await;
        }
    })
    .await
    .expect("后台清理任务应已启动");
    assert_eq!(repository.nonce_prune_calls(), 1);
}

#[actix_web::test]
async fn login_rechecks_nonce_expiration_after_signature_verification() {
    let issued_at = OffsetDateTime::from_unix_timestamp(1_752_372_000).unwrap();
    let repository = MemoryRepository::new();
    let clock: Arc<dyn Clock> = Arc::new(SequenceClock::new([
        issued_at,
        issued_at + Duration::minutes(4) + Duration::seconds(59),
        issued_at + Duration::minutes(5) + Duration::seconds(1),
    ]));
    let auth = AuthService::new(
        repository.clone(),
        Arc::new(AcceptVerifier),
        clock,
        Arc::new(FixedRandom {
            nonce: "freshTimeNonce1".to_owned(),
            token: "55".repeat(32),
        }),
        AuthConfig::local("127.0.0.1:5173", "http://127.0.0.1:5173"),
    );
    let nonce = auth.issue_nonce().await.unwrap();
    let message = siwe_message(
        &nonce.nonce,
        "0x0101010101010101010101010101010101010101",
        issued_at + Duration::minutes(4),
    );

    assert_eq!(
        auth.verify_and_create_session(&message, "0x01").await,
        Err(AuthError::NonceInvalid)
    );
    assert!(repository.account_for_wallet(1, [1_u8; 20]).is_none());
}

struct SequenceClock {
    times: Mutex<VecDeque<SystemTime>>,
}

impl SequenceClock {
    fn new(times: impl IntoIterator<Item = OffsetDateTime>) -> Self {
        Self {
            times: Mutex::new(times.into_iter().map(Into::into).collect()),
        }
    }
}

impl Clock for SequenceClock {
    fn monotonic_now(&self) -> StdDuration {
        StdDuration::ZERO
    }

    fn utc_now(&self) -> SystemTime {
        self.times.lock().unwrap().pop_front().unwrap_or(UNIX_EPOCH)
    }
}

async fn insert_nonce(
    repository: &MemoryRepository,
    nonce_hash: [u8; 32],
    expires_at: OffsetDateTime,
) {
    repository
        .insert_nonce(NewNonce {
            id: Uuid::new_v4(),
            nonce_hash,
            domain: "127.0.0.1:5173".to_owned(),
            uri: "http://127.0.0.1:5173".to_owned(),
            created_at: expires_at - Duration::minutes(1),
            expires_at,
        })
        .await
        .unwrap();
}
