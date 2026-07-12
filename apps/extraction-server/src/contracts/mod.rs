mod envelope;
mod manifest;

use std::{error::Error, fmt};

pub use envelope::{
    decode_protocol_envelope, EnvelopeFixture, EnvelopeFixtureCase, Outcome, ProtocolEnvelope,
};
pub use manifest::{
    EquipmentDefinition, EquipmentKey, ErrorCode, ExtractionManifest, ResourceDefinition,
    ResourceKey,
};

const MANIFEST_JSON: &str = include_str!("../../../../contracts/extraction/v1/manifest.json");
const ENVELOPES_JSON: &str =
    include_str!("../../../../contracts/extraction/v1/fixtures/envelopes.json");

pub fn bundled_manifest() -> Result<ExtractionManifest, ContractError> {
    let manifest: ExtractionManifest = serde_json::from_str(MANIFEST_JSON)
        .map_err(|error| ContractError::new(format!("manifest JSON 无效: {error}")))?;
    manifest.validate()?;
    Ok(manifest)
}

pub fn bundled_envelope_fixture() -> Result<EnvelopeFixture, ContractError> {
    serde_json::from_str(ENVELOPES_JSON)
        .map_err(|error| ContractError::new(format!("envelope fixture JSON 无效: {error}")))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractError {
    message: String,
}

impl ContractError {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ContractError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ContractError {}
