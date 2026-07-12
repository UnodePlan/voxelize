use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use super::{ContractError, ErrorCode, ExtractionManifest};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ProtocolEnvelope(WireProtocolEnvelope);

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
enum WireProtocolEnvelope {
    Intent {
        #[serde(rename = "protocolVersion")]
        protocol_version: u32,
        #[serde(rename = "requestId")]
        request_id: Uuid,
        sequence: u32,
        payload: Map<String, Value>,
    },
    Result {
        #[serde(rename = "protocolVersion")]
        protocol_version: u32,
        #[serde(rename = "requestId")]
        request_id: Uuid,
        outcome: Outcome,
    },
}

impl WireProtocolEnvelope {
    fn validate(&self, manifest: &ExtractionManifest) -> Result<(), ContractError> {
        let protocol_version = match self {
            Self::Intent {
                protocol_version, ..
            }
            | Self::Result {
                protocol_version, ..
            } => protocol_version,
        };
        if *protocol_version != manifest.protocol_version {
            return Err(ContractError::new("protocolVersion 不受支持"));
        }

        if let Self::Result {
            outcome: Outcome::Error { error },
            ..
        } = self
        {
            if !manifest.error_codes.contains(&error.code) {
                return Err(ContractError::new("结果包含未知错误码"));
            }
        }
        Ok(())
    }
}

pub fn decode_protocol_envelope(
    value: Value,
    manifest: &ExtractionManifest,
) -> Result<ProtocolEnvelope, ContractError> {
    let envelope: WireProtocolEnvelope = serde_json::from_value(value)
        .map_err(|error| ContractError::new(format!("protocol envelope JSON 无效: {error}")))?;
    envelope.validate(manifest)?;
    Ok(ProtocolEnvelope(envelope))
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "lowercase", deny_unknown_fields)]
pub enum Outcome {
    Ok { data: Value },
    Error { error: ErrorBody },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorBody {
    pub code: ErrorCode,
    pub retryable: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EnvelopeFixture {
    pub fixture_version: u32,
    pub cases: Vec<EnvelopeFixtureCase>,
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct EnvelopeFixtureCase {
    pub name: String,
    pub route: String,
    pub accept: bool,
    pub value: Value,
}
