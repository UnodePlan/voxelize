#![cfg(feature = "engine")]

mod support;

use std::sync::Arc;

use actix::Actor;
use actix_web::{
    http::{
        header::{HeaderMap, HeaderValue, COOKIE},
        StatusCode,
    },
    test, web, App,
};
use extraction_server::{
    auth::{AuthConfig, AuthService, SessionConnectionAuthenticator},
    configure_api,
    contracts::bundled_manifest,
    ports::Clock,
    AppState,
};
use serde_json::Value;
use time::OffsetDateTime;
use voxelize::{
    CloseAuthenticatedSession, Connect, ConnectionAuthErrorKind, ConnectionAuthRequest,
    ConnectionAuthenticator, ConnectionPrincipal, Server, WsSender,
};

use support::{
    empty_matchmaking, service_at, siwe_message, AcceptVerifier, FixedClock, FixedRandom,
    MemoryRepository,
};

#[actix_web::test]
async fn cookie_session_becomes_server_principal_and_revocation_denies_reuse() {
    let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
    let (auth, _repository) = service_at(now);
    let nonce = auth.issue_nonce().await.unwrap();
    let message = siwe_message(
        &nonce.nonce,
        "0x0000000000000000000000000000000000000001",
        now,
    );
    let created = auth
        .verify_and_create_session(&message, "0x01")
        .await
        .unwrap();
    let authenticator = SessionConnectionAuthenticator::new(auth.clone());
    let request = auth_request(auth.config().cookie_name(), &created.token);

    let principal = authenticator.authenticate(request.clone()).await.unwrap();
    let expected = ConnectionPrincipal::new(
        created.session.account_id.to_string(),
        created.session.session_id.to_string(),
    )
    .with_valid_until(created.session.valid_until().into());
    assert_eq!(principal, expected);
    assert_eq!(
        authenticator.revalidate(request.clone()).await.unwrap(),
        expected
    );

    assert_eq!(
        auth.revoke_token(Some(&created.token)).await.unwrap(),
        Some(created.session.session_id)
    );
    let error = authenticator.revalidate(request).await.unwrap_err();
    assert_eq!(error.kind, ConnectionAuthErrorKind::Unauthorized);
}

#[actix_web::test]
async fn connection_revalidation_does_not_extend_the_idle_deadline() {
    let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
    let (auth, repository) = service_at(now);
    let nonce = auth.issue_nonce().await.unwrap();
    let message = siwe_message(
        &nonce.nonce,
        "0x0000000000000000000000000000000000000001",
        now,
    );
    let created = auth
        .verify_and_create_session(&message, "0x01")
        .await
        .unwrap();
    let request = auth_request(auth.config().cookie_name(), &created.token);

    let mid_auth = auth_for(
        repository.clone(),
        now + time::Duration::hours(12),
        "midNonce12345678",
        &"33".repeat(32),
    );
    let expected = ConnectionPrincipal::new(
        created.session.account_id.to_string(),
        created.session.session_id.to_string(),
    )
    .with_valid_until(created.session.valid_until().into());
    assert_eq!(
        SessionConnectionAuthenticator::new(mid_auth)
            .revalidate(request.clone())
            .await
            .unwrap(),
        expected
    );

    let expired_auth = auth_for(
        repository,
        now + time::Duration::hours(25),
        "lateNonce1234567",
        &"44".repeat(32),
    );
    let error = SessionConnectionAuthenticator::new(expired_auth)
        .revalidate(request)
        .await
        .unwrap_err();
    assert_eq!(error.kind, ConnectionAuthErrorKind::Unauthorized);
}

#[actix_web::test]
async fn second_login_closes_the_websocket_of_the_replaced_session() {
    let now = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
    let (first_auth, repository) = service_at(now);
    let first_nonce = first_auth.issue_nonce().await.unwrap();
    let first_message = siwe_message(
        &first_nonce.nonce,
        "0x0000000000000000000000000000000000000001",
        now,
    );
    let first = first_auth
        .verify_and_create_session(&first_message, "0x01")
        .await
        .unwrap();
    let server = Server::new().debug(false).build().start();
    let (sender, _receiver) = WsSender::channel(1);
    server
        .send(Connect {
            id: None,
            principal: Some(ConnectionPrincipal::new(
                first.session.account_id.to_string(),
                first.session.session_id.to_string(),
            )),
            is_transport: false,
            sender,
        })
        .await
        .unwrap();

    let second_auth = auth_for(
        repository.clone(),
        now,
        "secondNonce123456",
        &"22".repeat(32),
    );
    let state = AppState::new(repository, bundled_manifest().unwrap()).with_services(
        second_auth,
        empty_matchmaking(Arc::new(FixedClock { now }) as Arc<dyn Clock>).await,
        Arc::new(FixedClock { now }) as Arc<dyn Clock>,
    );
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .app_data(web::Data::new(server.clone()))
            .configure(configure_api),
    )
    .await;
    let nonce = test::call_service(
        &app,
        test::TestRequest::get()
            .uri("/api/auth/siwe/nonce")
            .to_request(),
    )
    .await;
    let nonce: Value = test::read_body_json(nonce).await;
    let second_message = siwe_message(
        nonce["nonce"].as_str().unwrap(),
        "0x0000000000000000000000000000000000000001",
        now,
    );
    let verified = test::call_service(
        &app,
        test::TestRequest::post()
            .uri("/api/auth/siwe/verify")
            .set_json(serde_json::json!({
                "message": second_message,
                "signature": "0x01"
            }))
            .to_request(),
    )
    .await;
    assert_eq!(verified.status(), StatusCode::OK);

    let newly_closed = server
        .send(CloseAuthenticatedSession {
            session_id: first.session.session_id.to_string(),
        })
        .await
        .unwrap();
    assert_eq!(newly_closed, 0);
}

fn auth_for(
    repository: Arc<MemoryRepository>,
    now: OffsetDateTime,
    nonce: &str,
    token: &str,
) -> AuthService {
    AuthService::new(
        repository,
        Arc::new(AcceptVerifier),
        Arc::new(FixedClock { now }),
        Arc::new(FixedRandom {
            nonce: nonce.to_owned(),
            token: token.to_owned(),
        }),
        AuthConfig::local("127.0.0.1:5173", "http://127.0.0.1:5173"),
    )
}

fn auth_request(cookie_name: &str, token: &str) -> ConnectionAuthRequest {
    let mut headers = HeaderMap::new();
    headers.insert(
        COOKIE,
        HeaderValue::from_str(&format!("{cookie_name}={token}")).unwrap(),
    );
    ConnectionAuthRequest {
        headers,
        peer_addr: None,
        path: "/ws/".to_owned(),
    }
}
