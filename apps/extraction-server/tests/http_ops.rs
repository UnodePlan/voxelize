use std::{
    future::pending,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

use actix_web::{
    http::{header, StatusCode},
    test, web, App,
};
use async_trait::async_trait;
use extraction_server::ops::{
    configure_ops, OpsAccount, OpsAuditEvent, OpsAuditOutcome, OpsAuditSink, OpsHttpState,
    OpsLedgerEntry, OpsMatch, OpsPage, OpsPageRequest, OpsParticipant, OpsRepository,
    OpsRepositoryError, OpsResourceCounts, OpsResourceQuantity, OpsSettlement, OpsStateError,
    OpsWarehouse,
};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

const TOKEN: &str = "stage10-ops-token-is-at-least-32-bytes";
const ACCOUNT_ID: Uuid = Uuid::from_u128(10);
const MATCH_ID: Uuid = Uuid::from_u128(20);
const SETTLEMENT_ID: Uuid = Uuid::from_u128(30);

macro_rules! authorized_request {
    ($request:expr) => {
        $request
            .insert_header((header::AUTHORIZATION, format!("Bearer {TOKEN}")))
            .to_request()
    };
}

macro_rules! authorized_get {
    ($app:expr, $uri:expr $(,)?) => {{
        let response = test::call_service(
            $app,
            authorized_request!(test::TestRequest::get().uri($uri)),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CACHE_CONTROL)
                .unwrap()
                .to_str()
                .unwrap(),
            "no-store"
        );
        test::read_body_json::<Value, _>(response).await
    }};
}

#[actix_web::test]
async fn state_rejects_weak_credentials_and_unbounded_policies() {
    let repository = Arc::new(FixtureRepository::default());
    let audit = Arc::new(MemoryAudit::default());
    assert!(matches!(
        OpsHttpState::new(repository.clone(), "short", audit.clone()),
        Err(OpsStateError::WeakToken)
    ));
    let state = OpsHttpState::new(repository, TOKEN, audit).unwrap();
    assert!(matches!(
        state.with_policy(Duration::from_secs(6), 60),
        Err(OpsStateError::InvalidPolicy)
    ));
}

#[actix_web::test]
async fn authorized_fixed_queries_return_redacted_read_models_and_audit() {
    let repository = Arc::new(FixtureRepository::default());
    let audit = Arc::new(MemoryAudit::default());
    let state = OpsHttpState::new(repository.clone(), TOKEN, audit.clone()).unwrap();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .configure(configure_ops),
    )
    .await;

    let account = authorized_get!(&app, &format!("/ops/v1/accounts/{ACCOUNT_ID}"));
    assert_eq!(account["id"], ACCOUNT_ID.to_string());
    assert!(account.get("address").is_none());
    assert!(account.get("wallet").is_none());

    let match_record = authorized_get!(&app, &format!("/ops/v1/matches/{MATCH_ID}"));
    assert_eq!(match_record["state"], "finished");

    let participants = authorized_get!(
        &app,
        &format!("/ops/v1/matches/{MATCH_ID}/participants?limit=1&offset=0"),
    );
    assert_eq!(participants["items"].as_array().unwrap().len(), 1);
    assert_eq!(participants["nextOffset"], 1);

    let settlement = authorized_get!(&app, &format!("/ops/v1/settlements/{SETTLEMENT_ID}"),);
    assert_eq!(settlement["items"][0]["itemKey"], "gold");
    assert!(settlement.get("inventoryDigest").is_none());
    assert!(settlement.get("idempotencyKey").is_none());

    let warehouse = authorized_get!(&app, &format!("/ops/v1/accounts/{ACCOUNT_ID}/warehouse"),);
    assert_eq!(warehouse["balances"]["diamond"], 2);

    let ledger = authorized_get!(
        &app,
        &format!("/ops/v1/accounts/{ACCOUNT_ID}/ledger?limit=1"),
    );
    assert_eq!(ledger["items"][0]["delta"], 2);

    assert_eq!(repository.calls.load(Ordering::SeqCst), 6);
    let events = audit.events();
    assert_eq!(events.len(), 6);
    assert!(events
        .iter()
        .all(|event| event.outcome == OpsAuditOutcome::Success));
    let encoded = serde_json::to_string(&events).unwrap();
    assert!(!encoded.contains(TOKEN));
    assert!(!encoded.contains(&ACCOUNT_ID.to_string()));
    assert!(!encoded.contains(&MATCH_ID.to_string()));
}

#[actix_web::test]
async fn anonymous_player_cookie_and_wrong_token_are_rejected_without_queries() {
    let repository = Arc::new(FixtureRepository::default());
    let audit = Arc::new(MemoryAudit::default());
    let state = OpsHttpState::new(repository.clone(), TOKEN, audit.clone()).unwrap();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .configure(configure_ops),
    )
    .await;
    let uri = format!("/ops/v1/accounts/{ACCOUNT_ID}");

    let anonymous = test::call_service(&app, test::TestRequest::get().uri(&uri).to_request()).await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    assert!(anonymous.headers().contains_key(header::WWW_AUTHENTICATE));

    let player = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&uri)
            .insert_header((header::COOKIE, "voxel-extraction-session=player-token"))
            .to_request(),
    )
    .await;
    assert_eq!(player.status(), StatusCode::UNAUTHORIZED);

    let wrong = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&uri)
            .insert_header((header::AUTHORIZATION, "Bearer wrong-token"))
            .to_request(),
    )
    .await;
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(repository.calls.load(Ordering::SeqCst), 0);
    assert_eq!(audit.events().len(), 3);
}

#[actix_web::test]
async fn writes_arbitrary_queries_balance_changes_and_replays_are_rejected_and_audited() {
    let repository = Arc::new(FixtureRepository::default());
    let audit = Arc::new(MemoryAudit::default());
    let state = OpsHttpState::new(repository.clone(), TOKEN, audit.clone()).unwrap();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .configure(configure_ops),
    )
    .await;
    let requests = [
        authorized_request!(test::TestRequest::post()
            .uri(&format!("/ops/v1/accounts/{ACCOUNT_ID}"))
            .set_payload("{}")),
        authorized_request!(test::TestRequest::patch()
            .uri(&format!("/ops/v1/accounts/{ACCOUNT_ID}/warehouse"))
            .set_payload(r#"{"gold":999}"#)),
        authorized_request!(test::TestRequest::post()
            .uri("/ops/v1/query")
            .set_payload("UPDATE warehouse_balances SET quantity = 999")),
        authorized_request!(
            test::TestRequest::post().uri(&format!("/ops/v1/settlements/{SETTLEMENT_ID}/replay"))
        ),
    ];
    for request in requests {
        let response = test::call_service(&app, request).await;
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        let body: Value = test::read_body_json(response).await;
        assert_eq!(body["error"]["code"], "OPS_READ_ONLY");
    }
    assert_eq!(repository.calls.load(Ordering::SeqCst), 0);
    let events = audit.events();
    assert_eq!(events.len(), 4);
    assert!(events
        .iter()
        .all(|event| event.outcome == OpsAuditOutcome::MethodRejected));
}

#[actix_web::test]
async fn pagination_rate_limit_and_query_timeout_fail_closed() {
    let repository = Arc::new(FixtureRepository::default());
    let audit = Arc::new(MemoryAudit::default());
    let state = OpsHttpState::new(repository.clone(), TOKEN, audit.clone())
        .unwrap()
        .with_policy(Duration::from_secs(1), 2)
        .unwrap();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .configure(configure_ops),
    )
    .await;

    let invalid =
        authorized_request!(test::TestRequest::get()
            .uri(&format!("/ops/v1/accounts/{ACCOUNT_ID}/ledger?limit=101")));
    let invalid = test::call_service(&app, invalid).await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);

    let valid = authorized_request!(
        test::TestRequest::get().uri(&format!("/ops/v1/accounts/{ACCOUNT_ID}"))
    );
    assert_eq!(
        test::call_service(&app, valid).await.status(),
        StatusCode::OK
    );

    let limited = authorized_request!(
        test::TestRequest::get().uri(&format!("/ops/v1/accounts/{ACCOUNT_ID}"))
    );
    assert_eq!(
        test::call_service(&app, limited).await.status(),
        StatusCode::TOO_MANY_REQUESTS
    );

    let pending_repository = Arc::new(PendingRepository);
    let pending_audit = Arc::new(MemoryAudit::default());
    let pending_state = OpsHttpState::new(pending_repository, TOKEN, pending_audit.clone())
        .unwrap()
        .with_policy(Duration::from_millis(5), 10)
        .unwrap();
    let pending_app = test::init_service(
        App::new()
            .app_data(web::Data::new(pending_state))
            .configure(configure_ops),
    )
    .await;
    let request = authorized_request!(
        test::TestRequest::get().uri(&format!("/ops/v1/accounts/{ACCOUNT_ID}"))
    );
    let response = test::call_service(&pending_app, request).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        pending_audit.events()[0].outcome,
        OpsAuditOutcome::Unavailable
    );
}

#[derive(Default)]
struct MemoryAudit {
    events: Mutex<Vec<OpsAuditEvent>>,
}

impl MemoryAudit {
    fn events(&self) -> Vec<OpsAuditEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl OpsAuditSink for MemoryAudit {
    fn record(&self, event: OpsAuditEvent) {
        self.events.lock().unwrap().push(event);
    }
}

#[derive(Default)]
struct FixtureRepository {
    calls: AtomicUsize,
}

#[async_trait]
impl OpsRepository for FixtureRepository {
    async fn account(&self, account_id: Uuid) -> Result<Option<OpsAccount>, OpsRepositoryError> {
        self.called();
        Ok(Some(OpsAccount {
            id: account_id,
            status: "active".to_owned(),
            created_at: now(),
            updated_at: now(),
        }))
    }

    async fn match_record(&self, match_id: Uuid) -> Result<Option<OpsMatch>, OpsRepositoryError> {
        self.called();
        Ok(Some(OpsMatch {
            id: match_id,
            state: "finished".to_owned(),
            world_name: "match-test".to_owned(),
            seed: 42,
            generation_version: "generation-v1".to_owned(),
            gameplay_version: "gameplay-v1".to_owned(),
            config_version: "balance-v1".to_owned(),
            created_at: now(),
            started_at: Some(now()),
            extraction_open_at: Some(now()),
            hard_deadline: Some(now()),
            settlement_grace_deadline: Some(now()),
            finished_at: Some(now()),
            abort_reason: None,
        }))
    }

    async fn participants(
        &self,
        match_id: Uuid,
        page: OpsPageRequest,
    ) -> Result<OpsPage<OpsParticipant>, OpsRepositoryError> {
        self.called();
        assert_eq!(page, OpsPageRequest::new(1, 0).unwrap());
        Ok(OpsPage {
            items: vec![OpsParticipant {
                match_id,
                account_id: ACCOUNT_ID,
                public_player_id: Uuid::from_u128(11),
                seat_id: 0,
                state: "extracted".to_owned(),
                enqueued_at: now(),
                reconnect_deadline: None,
                killed_by_account_id: None,
                extracted_at: Some(now()),
                settlement_qualified_at: Some(now()),
                terminal_cause: None,
                terminal_at: None,
                survived_ms: None,
                mined: OpsResourceCounts::default(),
                picked_up: OpsResourceCounts::default(),
                lost: OpsResourceCounts::default(),
            }],
            next_offset: Some(1),
        })
    }

    async fn settlement(
        &self,
        settlement_id: Uuid,
    ) -> Result<Option<OpsSettlement>, OpsRepositoryError> {
        self.called();
        Ok(Some(OpsSettlement {
            id: settlement_id,
            match_id: MATCH_ID,
            account_id: ACCOUNT_ID,
            config_version: "balance-v1".to_owned(),
            total_value: 20,
            committed_at: now(),
            items: vec![OpsResourceQuantity {
                item_key: "gold".to_owned(),
                quantity: 2,
            }],
        }))
    }

    async fn warehouse(
        &self,
        account_id: Uuid,
    ) -> Result<Option<OpsWarehouse>, OpsRepositoryError> {
        self.called();
        Ok(Some(OpsWarehouse {
            account_id,
            balances: OpsResourceCounts {
                dirt: 3,
                gold: 2,
                diamond: 2,
            },
            updated_at: Some(now()),
        }))
    }

    async fn ledger(
        &self,
        account_id: Uuid,
        page: OpsPageRequest,
    ) -> Result<OpsPage<OpsLedgerEntry>, OpsRepositoryError> {
        self.called();
        assert_eq!(page.limit, 1);
        Ok(OpsPage {
            items: vec![OpsLedgerEntry {
                id: Uuid::from_u128(40),
                account_id,
                settlement_id: SETTLEMENT_ID,
                item_key: "diamond".to_owned(),
                delta: 2,
                created_at: now(),
            }],
            next_offset: None,
        })
    }
}

impl FixtureRepository {
    fn called(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

struct PendingRepository;

#[async_trait]
impl OpsRepository for PendingRepository {
    async fn account(&self, _id: Uuid) -> Result<Option<OpsAccount>, OpsRepositoryError> {
        pending().await
    }

    async fn match_record(&self, _id: Uuid) -> Result<Option<OpsMatch>, OpsRepositoryError> {
        pending().await
    }

    async fn participants(
        &self,
        _id: Uuid,
        _page: OpsPageRequest,
    ) -> Result<OpsPage<OpsParticipant>, OpsRepositoryError> {
        pending().await
    }

    async fn settlement(&self, _id: Uuid) -> Result<Option<OpsSettlement>, OpsRepositoryError> {
        pending().await
    }

    async fn warehouse(&self, _id: Uuid) -> Result<Option<OpsWarehouse>, OpsRepositoryError> {
        pending().await
    }

    async fn ledger(
        &self,
        _id: Uuid,
        _page: OpsPageRequest,
    ) -> Result<OpsPage<OpsLedgerEntry>, OpsRepositoryError> {
        pending().await
    }
}

fn now() -> OffsetDateTime {
    OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap()
}
