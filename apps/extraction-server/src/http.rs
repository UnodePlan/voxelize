use std::{sync::Arc, time::Duration};

use actix_web::{http::StatusCode, rt::time::timeout, web, HttpResponse};
use serde::{Deserialize, Serialize};

use crate::{contracts::ExtractionManifest, ports::RepositoryProbe};

const SERVICE_NAME: &str = "voxelize-extraction-server";
const DEFAULT_READINESS_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct AppState {
    repository: Arc<dyn RepositoryProbe>,
    manifest: ExtractionManifest,
    readiness_timeout: Duration,
}

impl AppState {
    pub fn new(repository: Arc<dyn RepositoryProbe>, manifest: ExtractionManifest) -> Self {
        Self {
            repository,
            manifest,
            readiness_timeout: DEFAULT_READINESS_TIMEOUT,
        }
    }

    pub fn with_readiness_timeout(mut self, readiness_timeout: Duration) -> Self {
        self.readiness_timeout = readiness_timeout;
        self
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub service: String,
    pub status: String,
}

pub fn configure_api(config: &mut web::ServiceConfig) {
    config
        .route("/health/live", web::get().to(liveness))
        .route("/health/ready", web::get().to(readiness))
        .route("/api/bootstrap", web::get().to(bootstrap));
}

async fn liveness() -> web::Json<HealthResponse> {
    web::Json(HealthResponse {
        service: SERVICE_NAME.to_owned(),
        status: "ok".to_owned(),
    })
}

async fn readiness(state: web::Data<AppState>) -> HttpResponse {
    let probe = timeout(state.readiness_timeout, state.repository.check()).await;
    let (status_code, status) = match probe {
        Ok(Ok(())) => (StatusCode::OK, "ready"),
        Ok(Err(_)) | Err(_) => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
    };
    HttpResponse::build(status_code).json(HealthResponse {
        service: SERVICE_NAME.to_owned(),
        status: status.to_owned(),
    })
}

async fn bootstrap(state: web::Data<AppState>) -> web::Json<ExtractionManifest> {
    web::Json(state.manifest.clone())
}
