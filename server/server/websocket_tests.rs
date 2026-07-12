use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use actix::Actor;
use actix_web::{http::StatusCode, test, web, App};

use crate::{ConnectionAuthError, ConnectionPrincipal, Server};

use super::super::ws_auth::AuthenticatedSessionGuard;
use super::*;

#[actix_web::test]
async fn empty_legacy_client_id_keeps_random_id_behavior() {
    let mut options = HashMap::new();
    options.insert("client_id".to_owned(), String::new());

    assert_eq!(requested_legacy_id(&options), None);
    options.insert("client_id".to_owned(), "legacy-player".to_owned());
    assert_eq!(
        requested_legacy_id(&options),
        Some("legacy-player".to_owned())
    );
}

#[actix_web::test]
async fn strict_origin_is_checked_before_authentication() {
    let auth_calls = Arc::new(AtomicUsize::new(0));
    let calls = auth_calls.clone();
    let http = HttpConfig::authenticated(move |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Ok(ConnectionPrincipal::new("account", "session")) }
    })
    .allowed_origins(["https://game.example"]);
    let server = Server::new().debug(false).build().start();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(server))
            .app_data(web::Data::new(HandshakeConfig { secret: None, http }))
            .route("/ws/", web::get().to(ws_route)),
    )
    .await;

    let request = test::TestRequest::get()
        .uri("/ws/?client_id=forged&secret=forged")
        .insert_header((header::ORIGIN, "https://evil.example"))
        .to_request();
    let response = test::call_service(&app, request).await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(auth_calls.load(Ordering::SeqCst), 0);
}

#[actix_web::test]
async fn strict_authentication_maps_failures_without_legacy_fallback() {
    let http =
        HttpConfig::authenticated(|_| async { Err(ConnectionAuthError::new("expired_session")) })
            .allowed_origins(["https://game.example"]);
    let server = Server::new().debug(false).build().start();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(server))
            .app_data(web::Data::new(HandshakeConfig {
                secret: Some("legacy-secret".to_owned()),
                http,
            }))
            .route("/ws/", web::get().to(ws_route)),
    )
    .await;

    let request = test::TestRequest::get()
        .uri("/ws/?client_id=forged&secret=legacy-secret")
        .insert_header((header::ORIGIN, "https://game.example"))
        .to_request();
    let response = test::call_service(&app, request).await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(test::read_body(response).await.is_empty());
}

#[actix_web::test]
async fn unavailable_authenticator_returns_service_unavailable() {
    let http = HttpConfig::authenticated(|_| async { Err(ConnectionAuthError::unavailable()) })
        .allowed_origins(["https://game.example"]);
    let server = Server::new().debug(false).build().start();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(server))
            .app_data(web::Data::new(HandshakeConfig { secret: None, http }))
            .route("/ws/", web::get().to(ws_route)),
    )
    .await;

    let request = test::TestRequest::get()
        .uri("/ws/")
        .insert_header((header::ORIGIN, "https://game.example"))
        .to_request();
    let response = test::call_service(&app, request).await;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[actix_web::test]
async fn authentication_timeout_fails_closed() {
    let http = HttpConfig::authenticated(|_| async {
        tokio::time::sleep(Duration::from_millis(25)).await;
        Ok(ConnectionPrincipal::new("account", "session"))
    })
    .allowed_origins(["https://game.example"])
    .auth_timeout(Duration::from_millis(1));
    let server = Server::new().debug(false).build().start();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(server))
            .app_data(web::Data::new(HandshakeConfig { secret: None, http }))
            .route("/ws/", web::get().to(ws_route)),
    )
    .await;

    let request = test::TestRequest::get()
        .uri("/ws/")
        .insert_header((header::ORIGIN, "https://game.example"))
        .to_request();
    let response = test::call_service(&app, request).await;

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[actix_web::test]
async fn duplicate_origin_is_rejected_before_authentication() {
    let auth_calls = Arc::new(AtomicUsize::new(0));
    let calls = auth_calls.clone();
    let http = HttpConfig::authenticated(move |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        async { Ok(ConnectionPrincipal::new("account", "session")) }
    })
    .allowed_origins(["https://game.example"]);
    let server = Server::new().debug(false).build().start();
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(server))
            .app_data(web::Data::new(HandshakeConfig { secret: None, http }))
            .route("/ws/", web::get().to(ws_route)),
    )
    .await;

    let request = test::TestRequest::get()
        .uri("/ws/")
        .append_header((header::ORIGIN, "https://game.example"))
        .append_header((header::ORIGIN, "https://game.example"))
        .to_request();
    let response = test::call_service(&app, request).await;

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(auth_calls.load(Ordering::SeqCst), 0);
}

#[actix_web::test]
async fn post_registration_revalidation_rejects_a_revoked_session() {
    let auth_calls = Arc::new(AtomicUsize::new(0));
    let calls = auth_calls.clone();
    let http = HttpConfig::authenticated(move |_| {
        let call = calls.fetch_add(1, Ordering::SeqCst);
        async move {
            if call == 0 {
                Ok(ConnectionPrincipal::new("account", "session"))
            } else {
                Err(ConnectionAuthError::new("revoked_session"))
            }
        }
    })
    .allowed_origins(["https://game.example"]);
    let request = ConnectionAuthRequest {
        headers: header::HeaderMap::new(),
        peer_addr: None,
        path: "/ws/".to_owned(),
    };
    let principal = http.authenticate(request.clone()).await.unwrap().unwrap();

    let mut guard = AuthenticatedSessionGuard::new(&http, request, principal);
    assert!(!guard.revalidate().await);
    assert_eq!(auth_calls.load(Ordering::SeqCst), 2);
}
