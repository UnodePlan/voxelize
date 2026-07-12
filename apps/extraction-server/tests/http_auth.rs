mod support;

use actix_web::{
    http::{header, StatusCode},
    test, web, App,
};
use extraction_server::{
    configure_api,
    contracts::bundled_manifest,
    matchmaking::MatchConnectionEvent,
    ports::{Clock, MatchRepository, WarehouseSnapshot, WarehouseStats},
    AppState,
};
use serde_json::Value;
use std::sync::Arc;
use time::OffsetDateTime;

use support::{empty_matchmaking, service_at, siwe_message, EmptyMatchRepository, FixedClock};

#[actix_web::test]
async fn empty_match_repository_has_no_startup_recovery_work() {
    assert_eq!(
        EmptyMatchRepository
            .abort_unrecoverable_matches("process_restart".to_owned(), OffsetDateTime::UNIX_EPOCH,)
            .await,
        Ok(0)
    );
}

#[actix_web::test]
async fn nonce_verify_session_warehouse_queue_and_logout_form_one_flow() {
    let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
    let (auth, repository) = service_at(now);
    let clock = Arc::new(FixedClock { now }) as Arc<dyn Clock>;
    let matchmaking = empty_matchmaking(clock.clone()).await;
    let state = AppState::new(repository.clone(), bundled_manifest().unwrap()).with_services(
        auth,
        matchmaking.clone(),
        clock,
    );
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .configure(configure_api),
    )
    .await;

    let nonce_response = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/auth/siwe/nonce")
            .to_request(),
    )
    .await;
    assert_eq!(nonce_response.status(), StatusCode::OK);
    assert!(nonce_response
        .headers()
        .get(header::CACHE_CONTROL)
        .unwrap()
        .to_str()
        .unwrap()
        .contains("no-store"));
    let nonce: Value = test::read_body_json(nonce_response).await;
    let nonce = nonce["nonce"].as_str().unwrap();
    let message = siwe_message(nonce, "0x0000000000000000000000000000000000000001", now);

    let verify = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/api/auth/siwe/verify")
            .set_json(serde_json::json!({
                "message": message,
                "signature": "0x01"
            }))
            .to_request(),
    )
    .await;
    assert_eq!(verify.status(), StatusCode::OK);
    assert!(verify
        .headers()
        .get(header::CACHE_CONTROL)
        .unwrap()
        .to_str()
        .unwrap()
        .contains("no-store"));
    let set_cookie = verify
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(set_cookie.starts_with("voxel-extraction-session="));
    assert!(set_cookie.contains("HttpOnly"));
    assert!(set_cookie.contains("SameSite=Lax"));
    assert!(set_cookie.contains("Path=/"));
    assert!(!set_cookie.contains("Secure"));
    let verified: bool = test::read_body_json(verify).await;
    assert!(verified);

    let session_cookie =
        "voxel-extraction-session=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let mut wallet_address = [0_u8; 20];
    wallet_address[19] = 1;
    let account_id = repository.account_for_wallet(1, wallet_address).unwrap();
    let queue_without_socket = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/api/matchmaking/queue")
            .insert_header((header::COOKIE, session_cookie))
            .to_request(),
    )
    .await;
    assert_eq!(queue_without_socket.status(), StatusCode::CONFLICT);
    let queue_without_socket: Value = test::read_body_json(queue_without_socket).await;
    assert_eq!(queue_without_socket["error"]["code"], "MATCH_ROSTER_LOCKED");
    matchmaking
        .apply_connection_event(MatchConnectionEvent::Connected {
            connection_id: "test-game-socket".to_owned(),
            account_id,
        })
        .await
        .unwrap();
    repository.seed_warehouse(
        account_id,
        WarehouseSnapshot {
            dirt: 127,
            gold: 64,
            diamond: 3,
            stats: WarehouseStats {
                total_resources_extracted: 194,
                total_extraction_value: 515,
                successful_extractions: 2,
                highest_single_match_value: 400,
            },
        },
    );
    let session = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/auth/session")
            .insert_header((header::COOKIE, session_cookie))
            .to_request(),
    )
    .await;
    assert_eq!(session.status(), StatusCode::OK);
    assert!(session
        .headers()
        .get(header::CACHE_CONTROL)
        .unwrap()
        .to_str()
        .unwrap()
        .contains("no-store"));
    let session: Value = test::read_body_json(session).await;
    assert_eq!(session["chainId"], 1);
    assert_eq!(
        session["address"],
        "0x0000000000000000000000000000000000000001"
    );

    let warehouse = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/warehouse")
            .insert_header((header::COOKIE, session_cookie))
            .to_request(),
    )
    .await;
    assert_eq!(warehouse.status(), StatusCode::OK);
    assert!(warehouse
        .headers()
        .get(header::CACHE_CONTROL)
        .unwrap()
        .to_str()
        .unwrap()
        .contains("no-store"));
    let warehouse: Value = test::read_body_json(warehouse).await;
    assert_eq!(warehouse["resources"]["dirt"], 127);
    assert_eq!(warehouse["resources"]["gold"], 64);
    assert_eq!(warehouse["resources"]["diamond"], 3);
    assert_eq!(warehouse["stats"]["totalResourcesExtracted"], 194);

    let queued = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/api/matchmaking/queue")
            .insert_header((header::COOKIE, session_cookie))
            .to_request(),
    )
    .await;
    assert_eq!(queued.status(), StatusCode::OK);
    let queued: Value = test::read_body_json(queued).await;
    assert_eq!(queued["position"], 1);

    let dequeued = test::call_service(
        &app,
        test::TestRequest::delete()
            .uri("/api/matchmaking/queue")
            .insert_header((header::COOKIE, session_cookie))
            .to_request(),
    )
    .await;
    assert_eq!(dequeued.status(), StatusCode::OK);
    let dequeued: Value = test::read_body_json(dequeued).await;
    assert_eq!(dequeued["status"], "idle");
    assert_eq!(dequeued["removed"], true);

    let logout = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/api/auth/logout")
            .insert_header((header::COOKIE, session_cookie))
            .to_request(),
    )
    .await;
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    assert!(logout.headers().contains_key(header::SET_COOKIE));

    let warehouse_after_logout = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/warehouse")
            .insert_header((header::COOKIE, session_cookie))
            .to_request(),
    )
    .await;
    assert_eq!(warehouse_after_logout.status(), StatusCode::UNAUTHORIZED);

    let session_after_logout = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/auth/session")
            .insert_header((header::COOKIE, session_cookie))
            .to_request(),
    )
    .await;
    assert_eq!(session_after_logout.status(), StatusCode::OK);
    let session_after_logout: Value = test::read_body_json(session_after_logout).await;
    assert!(session_after_logout.is_null());
}

#[actix_web::test]
async fn malformed_verify_and_duplicate_nonce_fail_closed() {
    let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
    let (auth, repository) = service_at(now);
    let state = AppState::new(repository, bundled_manifest().unwrap()).with_services(
        auth,
        empty_matchmaking(Arc::new(FixedClock { now }) as Arc<dyn Clock>).await,
        Arc::new(FixedClock { now }) as Arc<dyn Clock>,
    );
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .configure(configure_api),
    )
    .await;

    let malformed = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/api/auth/siwe/verify")
            .set_json(serde_json::json!({
                "message": "bad",
                "signature": "0x01",
                "unexpected": true
            }))
            .to_request(),
    )
    .await;
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);

    let nonce_response = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/auth/siwe/nonce")
            .to_request(),
    )
    .await;
    let nonce: Value = test::read_body_json(nonce_response).await;
    let message = siwe_message(
        nonce["nonce"].as_str().unwrap(),
        "0x0000000000000000000000000000000000000001",
        now,
    );
    let payload = serde_json::json!({"message": message, "signature": "0x01"});
    let first = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/api/auth/siwe/verify")
            .set_json(&payload)
            .to_request(),
    )
    .await;
    assert_eq!(first.status(), StatusCode::OK);
    let second = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/api/auth/siwe/verify")
            .set_json(payload)
            .to_request(),
    )
    .await;
    assert_eq!(second.status(), StatusCode::UNAUTHORIZED);
    let body: Value = test::read_body_json(second).await;
    assert_eq!(body["error"]["code"], "AUTH_NONCE_INVALID");
}

#[actix_web::test]
async fn rollout_flags_stop_new_login_and_queue_without_reviving_legacy_auth() {
    let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
    let (auth, repository) = service_at(now);
    let issued = auth.issue_nonce().await.unwrap();
    let message = siwe_message(
        &issued.nonce,
        "0x0000000000000000000000000000000000000001",
        now,
    );
    let created = auth
        .verify_and_create_session(&message, "0x01")
        .await
        .unwrap();
    let state = AppState::new(repository, bundled_manifest().unwrap())
        .with_services(
            auth,
            empty_matchmaking(Arc::new(FixedClock { now }) as Arc<dyn Clock>).await,
            Arc::new(FixedClock { now }) as Arc<dyn Clock>,
        )
        .with_feature_flags(false, false);
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .configure(configure_api),
    )
    .await;
    let session_cookie = format!("voxel-extraction-session={}", created.token);

    let nonce = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/auth/siwe/nonce")
            .to_request(),
    )
    .await;
    assert_eq!(nonce.status(), StatusCode::SERVICE_UNAVAILABLE);

    let queue = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/api/matchmaking/queue")
            .insert_header((header::COOKIE, session_cookie.as_str()))
            .to_request(),
    )
    .await;
    assert_eq!(queue.status(), StatusCode::SERVICE_UNAVAILABLE);

    let session = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/auth/session")
            .insert_header((header::COOKIE, session_cookie.as_str()))
            .to_request(),
    )
    .await;
    assert_eq!(session.status(), StatusCode::OK);

    let logout = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/api/auth/logout")
            .insert_header((header::COOKIE, session_cookie.as_str()))
            .to_request(),
    )
    .await;
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
}

#[actix_web::test]
async fn anonymous_auth_endpoints_are_bounded_before_database_or_rpc_work() {
    let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
    let (auth, repository) = service_at(now);
    let state = AppState::new(repository, bundled_manifest().unwrap()).with_services(
        auth,
        empty_matchmaking(Arc::new(FixedClock { now }) as Arc<dyn Clock>).await,
        Arc::new(FixedClock { now }) as Arc<dyn Clock>,
    );
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .configure(configure_api),
    )
    .await;

    for _ in 0..30 {
        let response = test::call_service(
            &app,
            test::TestRequest::get()
                .uri("/api/auth/siwe/nonce")
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    let limited = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/auth/siwe/nonce")
            .to_request(),
    )
    .await;
    assert_eq!(limited.status(), StatusCode::SERVICE_UNAVAILABLE);

    let invalid_payload = serde_json::json!({"message": "bad", "signature": "0x01"});
    for _ in 0..30 {
        let response = test::call_service(
            &app,
            test::TestRequest::post()
                .uri("/api/auth/siwe/verify")
                .set_json(&invalid_payload)
                .to_request(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let limited = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/api/auth/siwe/verify")
            .set_json(invalid_payload)
            .to_request(),
    )
    .await;
    assert_eq!(limited.status(), StatusCode::SERVICE_UNAVAILABLE);
}
