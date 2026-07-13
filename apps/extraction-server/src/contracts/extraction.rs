use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::{ContractError, ExtractionManifest};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtractionZoneState {
    pub center: [i32; 3],
    pub radius_blocks: u32,
    pub half_height_blocks: u32,
}

impl ExtractionZoneState {
    fn validate(self) -> Result<(), ContractError> {
        if self.radius_blocks == 0 || self.half_height_blocks == 0 {
            return Err(ContractError::new("extraction state 撤离区范围无效"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase", deny_unknown_fields)]
pub enum ExtractionStateData {
    Hidden {
        #[serde(rename = "extractionOpenAtUnixSeconds")]
        extraction_open_at_unix_seconds: u32,
        #[serde(rename = "hardDeadlineUnixSeconds")]
        hard_deadline_unix_seconds: u32,
    },
    Open {
        zone: ExtractionZoneState,
        inside: bool,
        #[serde(rename = "elapsedMs")]
        elapsed_ms: u32,
        #[serde(rename = "requiredMs")]
        required_ms: u32,
        #[serde(rename = "hardDeadlineUnixSeconds")]
        hard_deadline_unix_seconds: u32,
    },
    Pending {
        zone: ExtractionZoneState,
        #[serde(rename = "qualifiedAtUnixSeconds")]
        qualified_at_unix_seconds: u32,
    },
    Closed {},
}

impl ExtractionStateData {
    fn validate(self) -> Result<(), ContractError> {
        match self {
            Self::Hidden {
                extraction_open_at_unix_seconds,
                hard_deadline_unix_seconds,
            } => {
                if extraction_open_at_unix_seconds == 0
                    || extraction_open_at_unix_seconds >= hard_deadline_unix_seconds
                {
                    return Err(ContractError::new("extraction state 隐藏阶段绝对时间无效"));
                }
                Ok(())
            }
            Self::Closed {} => Ok(()),
            Self::Open {
                zone,
                elapsed_ms,
                required_ms,
                hard_deadline_unix_seconds,
                ..
            } => {
                zone.validate()?;
                if required_ms == 0 || elapsed_ms > required_ms || hard_deadline_unix_seconds == 0 {
                    return Err(ContractError::new("extraction state 进度或截止时间无效"));
                }
                Ok(())
            }
            Self::Pending {
                zone,
                qualified_at_unix_seconds,
            } => {
                zone.validate()?;
                if qualified_at_unix_seconds == 0 {
                    return Err(ContractError::new("extraction state 达标时间无效"));
                }
                Ok(())
            }
        }
    }

    pub const fn is_pending(self) -> bool {
        matches!(self, Self::Pending { .. })
    }

    pub const fn is_closed(self) -> bool {
        matches!(self, Self::Closed {})
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtractionStateEnvelope {
    protocol_version: u32,
    #[serde(rename = "type")]
    kind: ExtractionStateKind,
    pub match_id: Uuid,
    stream: ExtractionStateStream,
    pub revision: u32,
    pub data: ExtractionStateData,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum ExtractionStateKind {
    State,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum ExtractionStateStream {
    Extraction,
}

impl ExtractionStateEnvelope {
    pub fn new(
        manifest: &ExtractionManifest,
        match_id: Uuid,
        revision: u32,
        data: ExtractionStateData,
    ) -> Result<Self, ContractError> {
        let state = Self {
            protocol_version: manifest.protocol_version,
            kind: ExtractionStateKind::State,
            match_id,
            stream: ExtractionStateStream::Extraction,
            revision,
            data,
        };
        state.validate(manifest)?;
        Ok(state)
    }

    pub(crate) fn validate(&self, manifest: &ExtractionManifest) -> Result<(), ContractError> {
        if self.protocol_version != manifest.protocol_version || self.match_id.is_nil() {
            return Err(ContractError::new(
                "extraction state envelope 身份或版本无效",
            ));
        }
        self.data.validate()
    }
}

pub fn decode_extraction_state(
    value: Value,
    manifest: &ExtractionManifest,
) -> Result<ExtractionStateEnvelope, ContractError> {
    let state = serde_json::from_value::<ExtractionStateEnvelope>(value)
        .map_err(|error| ContractError::new(format!("extraction state JSON 无效: {error}")))?;
    state.validate(manifest)?;
    Ok(state)
}
