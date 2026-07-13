use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use super::{ContractError, ErrorCode, ExtractionManifest};

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ProtocolEnvelope(WireProtocolEnvelope);

#[derive(Clone, Debug, PartialEq)]
pub struct Intent<T> {
    pub request_id: Uuid,
    pub sequence: u32,
    pub payload: T,
}

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

impl ProtocolEnvelope {
    pub fn request_id(&self) -> Uuid {
        match &self.0 {
            WireProtocolEnvelope::Intent { request_id, .. }
            | WireProtocolEnvelope::Result { request_id, .. } => *request_id,
        }
    }

    pub fn decode_intent<T: DeserializeOwned>(&self) -> Result<Intent<T>, ContractError> {
        let WireProtocolEnvelope::Intent {
            request_id,
            sequence,
            payload,
            ..
        } = &self.0
        else {
            return Err(ContractError::new("期望 intent envelope"));
        };
        let payload = serde_json::from_value(Value::Object(payload.clone()))
            .map_err(|error| ContractError::new(format!("intent payload JSON 无效: {error}")))?;
        Ok(Intent {
            request_id: *request_id,
            sequence: *sequence,
            payload,
        })
    }

    pub fn ok<T: Serialize>(
        manifest: &ExtractionManifest,
        request_id: Uuid,
        data: T,
    ) -> Result<Self, ContractError> {
        let data = serde_json::to_value(data)
            .map_err(|error| ContractError::new(format!("result data JSON 无效: {error}")))?;
        Ok(Self(WireProtocolEnvelope::Result {
            protocol_version: manifest.protocol_version,
            request_id,
            outcome: Outcome::Ok { data },
        }))
    }

    pub fn error(
        manifest: &ExtractionManifest,
        request_id: Uuid,
        code: ErrorCode,
        retryable: bool,
    ) -> Result<Self, ContractError> {
        if !manifest.error_codes.contains(&code) {
            return Err(ContractError::new("结果包含未知错误码"));
        }
        Ok(Self(WireProtocolEnvelope::Result {
            protocol_version: manifest.protocol_version,
            request_id,
            outcome: Outcome::Error {
                error: ErrorBody { code, retryable },
            },
        }))
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
