mod support;

use std::sync::Arc;

use actix_web::{
    http::{header, StatusCode},
    test, web, App,
};
use extraction_server::{
    configure_api,
    contracts::bundled_manifest,
    matchmaking::{
        MatchResultRecord, MatchState, MatchVersions, MatchmakingService, ParticipantMatchStats,
        ParticipantResourceCounts, ParticipantState, ParticipantTerminalCause, SettlementRecord,
        SettlementResources,
    },
    ports::{Clock, RandomIdGenerator, RandomSeedGenerator},
    AppState,
};
use serde_json::Value;
use time::OffsetDateTime;
use uuid::Uuid;

use support::{service_at, siwe_message, EmptyMatchRepository, FixedClock};

macro_rules! request_json {
    ($app:expr, $uri:expr, $cookie:expr) => {{
        let response = test::call_service(
            $app,
            test::TestRequest::get()
                .uri($uri)
                .insert_header((header::COOKIE, ($cookie).as_str()))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = test::read_body_json(response).await;
        body
    }};
}

#[actix_web::test]
async fn result_routes_bind_queries_to_the_authenticated_account() {
    let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
    let (auth, auth_repository) = service_at(now);
    let issued = auth.issue_nonce().await.unwrap();
    let message = siwe_message(
        &issued.nonce,
        "0x0000000000000000000000000000000000000001",
        now,
    );
    let session = auth
        .verify_and_create_session(&message, "0x01")
        .await
        .unwrap();
    let mut address = [0_u8; 20];
    address[19] = 1;
    let account_id = auth_repository.account_for_wallet(1, address).unwrap();
    let other_account_id = Uuid::from_u128(9002);

    let result_repository = Arc::new(EmptyMatchRepository::default());
    let active = result_record(
        Uuid::from_u128(9100),
        ParticipantState::Active,
        MatchState::Active,
    );
    result_repository.seed_match_result(account_id, active.clone());
    let corrupt_match_id = Uuid::from_u128(9106);
    result_repository.seed_match_result(
        account_id,
        result_record(
            corrupt_match_id,
            ParticipantState::Dead,
            MatchState::Finished,
        ),
    );
    let results = result_records(account_id, now);
    for result in &results {
        result_repository.seed_match_result(account_id, result.clone());
    }
    let other_match_id = Uuid::from_u128(9900);
    result_repository.seed_match_result(
        other_account_id,
        result_record(other_match_id, ParticipantState::Dead, MatchState::Finished),
    );

    let clock = Arc::new(FixedClock { now }) as Arc<dyn Clock>;
    let matchmaking = MatchmakingService::start(
        result_repository,
        clock.clone(),
        Arc::new(RandomIdGenerator),
        Arc::new(RandomSeedGenerator),
        MatchVersions {
            generation: "generation-v1".to_owned(),
            gameplay: "gameplay-v1".to_owned(),
            config: "balance-v1".to_owned(),
        },
    );
    let state = AppState::new(auth_repository, bundled_manifest().unwrap()).with_services(
        auth,
        matchmaking,
        clock,
    );
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .configure(configure_api),
    )
    .await;
    let cookie = format!("voxel-extraction-session={}", session.token);

    let anonymous = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&format!("/api/matches/{}/result", results[0].match_id))
            .to_request(),
    )
    .await;
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    assert!(anonymous
        .headers()
        .get(header::CACHE_CONTROL)
        .unwrap()
        .to_str()
        .unwrap()
        .contains("no-store"));

    let malformed = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/matches/not-a-uuid/result")
            .insert_header((header::COOKIE, cookie.as_str()))
            .to_request(),
    )
    .await;
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    assert!(malformed.headers().contains_key(header::CACHE_CONTROL));
    let malformed: Value = test::read_body_json(malformed).await;
    assert_eq!(malformed["error"]["code"], "REQUEST_MALFORMED");

    let active_result = request_json!(
        &app,
        &format!("/api/matches/{}/result", active.match_id),
        &cookie
    );
    assert_eq!(active_result["status"], "pendingReconciliation");

    let expected_statuses = [
        "pendingReconciliation",
        "extracted",
        "dead",
        "timedOut",
        "aborted",
    ];
    for (record, expected_status) in results.iter().zip(expected_statuses) {
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri(&format!("/api/matches/{}/result", record.match_id))
                .insert_header((header::COOKIE, cookie.as_str()))
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get(header::CACHE_CONTROL)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("no-store"));
        let body: Value = test::read_body_json(response).await;
        assert_eq!(body["status"], expected_status);
        assert!(body.get("accountId").is_none());
        assert!(body.get("inventoryDigest").is_none());
        assert!(body.get("idempotencyKey").is_none());
    }

    let extracted = request_json!(
        &app,
        &format!("/api/matches/{}/result", results[1].match_id),
        &cookie
    );
    assert_eq!(extracted["settlement"]["resources"]["diamond"], 2);
    assert_eq!(extracted["settlement"]["totalValue"], 225);
    assert_eq!(extracted["settlement"]["configVersion"], "balance-v1");

    let dead = request_json!(
        &app,
        &format!("/api/matches/{}/result", results[2].match_id),
        &cookie
    );
    assert_eq!(dead["terminalCause"], "melee");
    assert_eq!(dead["survivedMs"], 75_000);
    assert_eq!(dead["stats"]["mined"]["gold"], 3);
    assert_eq!(dead["stats"]["pickedUp"]["diamond"], 1);
    assert_eq!(dead["stats"]["lost"]["dirt"], 14);

    let other_result = request_json!(
        &app,
        &format!("/api/matches/{other_match_id}/result?accountId={other_account_id}"),
        &cookie
    );
    assert!(other_result.is_null());

    let latest = request_json!(&app, "/api/matches/latest-result", &cookie);
    assert_eq!(latest["matchId"], results[4].match_id.to_string());
    assert_eq!(latest["status"], "aborted");

    let corrupt = test::call_service(
        &app,
        test::TestRequest::get()
            .uri(&format!("/api/matches/{corrupt_match_id}/result"))
            .insert_header((header::COOKIE, cookie.as_str()))
            .to_request(),
    )
    .await;
    assert_eq!(corrupt.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(corrupt.headers().contains_key(header::CACHE_CONTROL));
}

fn result_records(account_id: Uuid, now: OffsetDateTime) -> Vec<MatchResultRecord> {
    let mut pending = result_record(
        Uuid::from_u128(9101),
        ParticipantState::SettlementPending,
        MatchState::Settling,
    );
    pending.public_player_id = Uuid::from_u128(9201);

    let mut extracted = result_record(
        Uuid::from_u128(9102),
        ParticipantState::Extracted,
        MatchState::Finished,
    );
    extracted.settlement = Some(SettlementRecord {
        settlement_id: Uuid::from_u128(9302),
        match_id: extracted.match_id,
        account_id,
        idempotency_key: format!("extract:v1:{}:{account_id}", extracted.match_id),
        inventory_digest: [7; 32],
        config_version: "balance-v1".to_owned(),
        resources: SettlementResources::new(5, 2, 2),
        total_value: 225,
        committed_at: now,
    });

    let mut dead = result_record(
        Uuid::from_u128(9103),
        ParticipantState::Dead,
        MatchState::ExtractionOpen,
    );
    dead.terminal_cause = Some(ParticipantTerminalCause::Melee);
    dead.killer_public_player_id = Some(Uuid::from_u128(9403));
    dead.terminal_at = Some(now);
    dead.survived_ms = Some(75_000);
    dead.stats = test_stats();

    let mut timed_out = result_record(
        Uuid::from_u128(9104),
        ParticipantState::TimedOut,
        MatchState::Finished,
    );
    timed_out.terminal_cause = Some(ParticipantTerminalCause::HardDeadline);
    timed_out.terminal_at = Some(now);
    timed_out.survived_ms = Some(720_000);
    timed_out.stats = test_stats();

    let aborted = result_record(
        Uuid::from_u128(9105),
        ParticipantState::Aborted,
        MatchState::Aborted,
    );
    vec![pending, extracted, dead, timed_out, aborted]
}

fn result_record(
    match_id: Uuid,
    participant_state: ParticipantState,
    match_state: MatchState,
) -> MatchResultRecord {
    MatchResultRecord {
        match_id,
        match_state,
        participant_state,
        public_player_id: Uuid::from_u128(match_id.as_u128() + 100),
        terminal_cause: None,
        killer_public_player_id: None,
        terminal_at: None,
        survived_ms: None,
        stats: ParticipantMatchStats::default(),
        settlement: None,
        abort_reason: None,
    }
}

fn test_stats() -> ParticipantMatchStats {
    ParticipantMatchStats {
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
    }
}
