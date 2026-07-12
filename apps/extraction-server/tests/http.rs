use std::{future::pending, sync::Arc, time::Duration};

use actix_web::{http::StatusCode, test, web, App};
use extraction_server::{
    configure_api,
    contracts::{bundled_manifest, ExtractionManifest},
    ports::{RepositoryError, RepositoryFuture, RepositoryProbe},
    AppState, HealthResponse,
};

#[actix_web::test]
async fn liveness_readiness_and_bootstrap_are_available() {
    let manifest = bundled_manifest().unwrap();
    let state = web::Data::new(AppState::new(Arc::new(ReadyProbe), manifest.clone()));
    let app = test::init_service(App::new().app_data(state).configure(configure_api)).await;

    let live: HealthResponse = test::call_and_read_body_json(
        &app,
        test::TestRequest::get().uri("/health/live").to_request(),
    )
    .await;
    assert_eq!(live.status, "ok");

    let ready: HealthResponse = test::call_and_read_body_json(
        &app,
        test::TestRequest::get().uri("/health/ready").to_request(),
    )
    .await;
    assert_eq!(ready.status, "ready");

    let response: ExtractionManifest = test::call_and_read_body_json(
        &app,
        test::TestRequest::get().uri("/api/bootstrap").to_request(),
    )
    .await;
    assert_eq!(response, manifest);
}

#[actix_web::test]
async fn readiness_fails_closed_when_repository_is_unavailable() {
    let state = web::Data::new(AppState::new(
        Arc::new(FailingProbe),
        bundled_manifest().unwrap(),
    ));
    let app = test::init_service(App::new().app_data(state).configure(configure_api)).await;

    let response = test::call_service(
        &app,
        test::TestRequest::get().uri("/health/ready").to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[actix_web::test]
async fn readiness_fails_closed_when_repository_probe_times_out() {
    let state = AppState::new(Arc::new(PendingProbe), bundled_manifest().unwrap())
        .with_readiness_timeout(Duration::from_millis(5));
    let app = test::init_service(
        App::new()
            .app_data(web::Data::new(state))
            .configure(configure_api),
    )
    .await;

    let response = test::call_service(
        &app,
        test::TestRequest::get().uri("/health/ready").to_request(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

struct ReadyProbe;

impl RepositoryProbe for ReadyProbe {
    fn check(&self) -> RepositoryFuture<'_> {
        Box::pin(async { Ok(()) })
    }
}

struct FailingProbe;

impl RepositoryProbe for FailingProbe {
    fn check(&self) -> RepositoryFuture<'_> {
        Box::pin(async { Err(RepositoryError::new("test repository unavailable")) })
    }
}

struct PendingProbe;

impl RepositoryProbe for PendingProbe {
    fn check(&self) -> RepositoryFuture<'_> {
        Box::pin(pending())
    }
}
