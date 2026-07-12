use actix_web::{
    http::header::{CacheControl, CacheDirective},
    web, HttpRequest, HttpResponse,
};
use serde::Serialize;

use super::{error::ApiError, session::required_session, AppState};

pub(super) fn configure(config: &mut web::ServiceConfig) {
    config.route("/api/warehouse", web::get().to(warehouse));
}

async fn warehouse(
    request: HttpRequest,
    state: web::Data<AppState>,
) -> Result<HttpResponse, ApiError> {
    let session = required_session(&request, &state).await?;
    let auth = state.auth().ok_or_else(ApiError::service_unavailable)?;
    let snapshot = auth
        .warehouse(session.account_id)
        .await
        .map_err(ApiError::from)?;
    Ok(HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoStore]))
        .json(WarehouseResponse {
            resources: ResourceBalances {
                dirt: snapshot.dirt,
                gold: snapshot.gold,
                diamond: snapshot.diamond,
            },
            stats: WarehouseStatsResponse {
                total_resources_extracted: snapshot.stats.total_resources_extracted,
                total_extraction_value: snapshot.stats.total_extraction_value,
                successful_extractions: snapshot.stats.successful_extractions,
                highest_single_match_value: snapshot.stats.highest_single_match_value,
            },
        }))
}

#[derive(Serialize)]
struct WarehouseResponse {
    resources: ResourceBalances,
    stats: WarehouseStatsResponse,
}

#[derive(Serialize)]
struct ResourceBalances {
    dirt: i64,
    gold: i64,
    diamond: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WarehouseStatsResponse {
    total_resources_extracted: i64,
    total_extraction_value: i64,
    successful_extractions: i64,
    highest_single_match_value: i64,
}
