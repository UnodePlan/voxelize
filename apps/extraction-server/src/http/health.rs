use actix_web::{http::StatusCode, rt::time::timeout, web, HttpResponse};
use serde::{Deserialize, Serialize};

use super::AppState;

const SERVICE_NAME: &str = "voxelize-extraction-server";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub service: String,
    pub status: String,
}

pub(super) fn configure(config: &mut web::ServiceConfig) {
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

async fn bootstrap(state: web::Data<AppState>) -> web::Json<crate::contracts::ExtractionManifest> {
    web::Json(state.manifest.clone())
}
