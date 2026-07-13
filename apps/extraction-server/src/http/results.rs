use actix_web::{
    http::header::{CacheControl, CacheDirective},
    web, HttpRequest, HttpResponse,
};
use serde::Serialize;
use time::format_description::well_known::Rfc3339;
use uuid::Uuid;

use super::{error::ApiError, session::required_session, AppState};
use crate::matchmaking::{
    MatchResultRecord, MatchState, ParticipantMatchStats, ParticipantResourceCounts,
    ParticipantState, ParticipantTerminalCause, SettlementRecord, SettlementResources,
};

pub(super) fn configure(config: &mut web::ServiceConfig) {
    config
        .route(
            "/api/matches/{match_id}/result",
            web::get().to(match_result),
        )
        .route(
            "/api/matches/latest-result",
            web::get().to(latest_match_result),
        );
}

async fn match_result(
    request: HttpRequest,
    state: web::Data<AppState>,
    match_id: web::Path<String>,
) -> Result<HttpResponse, ApiError> {
    let session = required_session(&request, &state).await?;
    let match_id = Uuid::parse_str(&match_id).map_err(|_| ApiError::request_malformed())?;
    let matchmaking = state
        .matchmaking()
        .ok_or_else(ApiError::service_unavailable)?;
    let result = matchmaking
        .find_match_result(match_id, session.account_id)
        .await
        .map_err(ApiError::from)?;
    result_response(result, session.account_id)
}

async fn latest_match_result(
    request: HttpRequest,
    state: web::Data<AppState>,
) -> Result<HttpResponse, ApiError> {
    let session = required_session(&request, &state).await?;
    let matchmaking = state
        .matchmaking()
        .ok_or_else(ApiError::service_unavailable)?;
    let result = matchmaking
        .find_latest_match_result(session.account_id)
        .await
        .map_err(ApiError::from)?;
    result_response(result, session.account_id)
}

fn result_response(
    result: Option<MatchResultRecord>,
    account_id: Uuid,
) -> Result<HttpResponse, ApiError> {
    let result = match result {
        Some(record) => MatchResultResponse::try_from_record(record, account_id)?,
        None => None,
    };
    Ok(HttpResponse::Ok()
        .insert_header(CacheControl(vec![CacheDirective::NoStore]))
        .json(result))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MatchResultResponse {
    match_id: Uuid,
    status: MatchResultStatus,
    public_player_id: Uuid,
    terminal_cause: Option<TerminalCauseResponse>,
    killer_public_player_id: Option<Uuid>,
    terminal_at: Option<String>,
    survived_ms: Option<u32>,
    stats: MatchStatsResponse,
    settlement: Option<SettlementResponse>,
}

impl MatchResultResponse {
    fn try_from_record(
        record: MatchResultRecord,
        account_id: Uuid,
    ) -> Result<Option<Self>, ApiError> {
        let status = result_status(record.participant_state, record.match_state);
        let terminal_at = record
            .terminal_at
            .map(|value| value.format(&Rfc3339))
            .transpose()
            .map_err(|_| ApiError::service_unavailable())?;
        if record.settlement.as_ref().is_some_and(|settlement| {
            settlement.match_id != record.match_id || settlement.account_id != account_id
        }) {
            return Err(ApiError::service_unavailable());
        }
        let settlement = record
            .settlement
            .map(SettlementResponse::try_from)
            .transpose()?;
        if (status == MatchResultStatus::Extracted) != settlement.is_some() {
            return Err(ApiError::service_unavailable());
        }
        validate_terminal_shape(
            status,
            record.terminal_cause,
            record.killer_public_player_id,
            record.terminal_at.is_some(),
            record.survived_ms.is_some(),
        )?;
        Ok(Some(Self {
            match_id: record.match_id,
            status,
            public_player_id: record.public_player_id,
            terminal_cause: record.terminal_cause.map(Into::into),
            killer_public_player_id: record.killer_public_player_id,
            terminal_at,
            survived_ms: record.survived_ms,
            stats: record.stats.into(),
            settlement,
        }))
    }
}

fn validate_terminal_shape(
    status: MatchResultStatus,
    cause: Option<ParticipantTerminalCause>,
    killer: Option<Uuid>,
    has_terminal_at: bool,
    has_survived_ms: bool,
) -> Result<(), ApiError> {
    let valid = match status {
        MatchResultStatus::Dead => {
            cause == Some(ParticipantTerminalCause::Melee)
                && killer.is_some_and(|value| !value.is_nil())
                && has_terminal_at
                && has_survived_ms
        }
        MatchResultStatus::TimedOut => {
            matches!(
                cause,
                Some(
                    ParticipantTerminalCause::ReconnectTimeout
                        | ParticipantTerminalCause::HardDeadline
                )
            ) && killer.is_none()
                && has_terminal_at
                && has_survived_ms
        }
        MatchResultStatus::PendingReconciliation
        | MatchResultStatus::Extracted
        | MatchResultStatus::Aborted => {
            cause.is_none() && killer.is_none() && !has_terminal_at && !has_survived_ms
        }
    };
    valid
        .then_some(())
        .ok_or_else(ApiError::service_unavailable)
}

fn result_status(participant: ParticipantState, match_state: MatchState) -> MatchResultStatus {
    match participant {
        ParticipantState::SettlementPending => MatchResultStatus::PendingReconciliation,
        ParticipantState::Extracted => MatchResultStatus::Extracted,
        ParticipantState::Dead => MatchResultStatus::Dead,
        ParticipantState::TimedOut => MatchResultStatus::TimedOut,
        ParticipantState::Aborted => MatchResultStatus::Aborted,
        _ if match_state == MatchState::Aborted => MatchResultStatus::Aborted,
        _ => MatchResultStatus::PendingReconciliation,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
enum MatchResultStatus {
    PendingReconciliation,
    Extracted,
    Dead,
    TimedOut,
    Aborted,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
enum TerminalCauseResponse {
    Melee,
    ReconnectTimeout,
    HardDeadline,
}

impl From<ParticipantTerminalCause> for TerminalCauseResponse {
    fn from(value: ParticipantTerminalCause) -> Self {
        match value {
            ParticipantTerminalCause::Melee => Self::Melee,
            ParticipantTerminalCause::ReconnectTimeout => Self::ReconnectTimeout,
            ParticipantTerminalCause::HardDeadline => Self::HardDeadline,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MatchStatsResponse {
    mined: ResourceCountsResponse,
    picked_up: ResourceCountsResponse,
    lost: ResourceCountsResponse,
}

impl From<ParticipantMatchStats> for MatchStatsResponse {
    fn from(value: ParticipantMatchStats) -> Self {
        Self {
            mined: value.mined.into(),
            picked_up: value.picked_up.into(),
            lost: value.lost.into(),
        }
    }
}

#[derive(Serialize)]
struct ResourceCountsResponse {
    dirt: u64,
    gold: u64,
    diamond: u64,
}

impl From<ParticipantResourceCounts> for ResourceCountsResponse {
    fn from(value: ParticipantResourceCounts) -> Self {
        Self {
            dirt: value.dirt,
            gold: value.gold,
            diamond: value.diamond,
        }
    }
}

impl From<SettlementResources> for ResourceCountsResponse {
    fn from(value: SettlementResources) -> Self {
        Self {
            dirt: value.dirt,
            gold: value.gold,
            diamond: value.diamond,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettlementResponse {
    settlement_id: Uuid,
    resources: ResourceCountsResponse,
    total_value: i64,
    config_version: String,
    committed_at: String,
}

impl TryFrom<SettlementRecord> for SettlementResponse {
    type Error = ApiError;

    fn try_from(value: SettlementRecord) -> Result<Self, Self::Error> {
        Ok(Self {
            settlement_id: value.settlement_id,
            resources: value.resources.into(),
            total_value: value.total_value,
            config_version: value.config_version,
            committed_at: value
                .committed_at
                .format(&Rfc3339)
                .map_err(|_| ApiError::service_unavailable())?,
        })
    }
}
