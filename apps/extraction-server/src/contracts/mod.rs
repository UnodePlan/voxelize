mod combat;
mod envelope;
mod extraction;
mod gameplay;
mod gameplay_state;
mod manifest;

use std::{error::Error, fmt};

pub use combat::{
    decode_attack_intent, decode_attack_result_data, decode_death_result, decode_health_state,
    AttackPayload, AttackResolution, AttackResultData, AttackWeaponSlot, DeathCause,
    DeathResultData, DeathResultEnvelope, HealthStateData, HealthStateEnvelope, ResourceTally,
    MAX_HALF_HEARTS,
};
pub use envelope::{
    decode_protocol_envelope, EnvelopeFixture, EnvelopeFixtureCase, Intent, Outcome,
    ProtocolEnvelope,
};
pub use extraction::{
    decode_extraction_state, ExtractionStateData, ExtractionStateEnvelope, ExtractionZoneState,
};
pub use gameplay::{
    decode_drop_slot_intent, decode_mining_intent, decode_mining_state, DropSlotPayload,
    MiningIdleReason, MiningPayload, MiningStateData, MiningStateEnvelope,
};
pub use gameplay_state::{
    decode_gameplay_state, decode_get_state_intent, AttackCursorState, FixedEquipmentState,
    GameplayStateData, GetStatePayload, InventoryState, ResourceStackState,
};
pub use manifest::{
    EquipmentDefinition, EquipmentKey, ErrorCode, ExtractionManifest, ResourceDefinition,
    ResourceKey,
};

const MANIFEST_JSON: &str = include_str!("../../../../contracts/extraction/v1/manifest.json");
const ENVELOPES_JSON: &str =
    include_str!("../../../../contracts/extraction/v1/fixtures/envelopes.json");
const GAMEPLAY_INTENTS_JSON: &str =
    include_str!("../../../../contracts/extraction/v1/fixtures/gameplay-intents.json");
const MINING_STATES_JSON: &str =
    include_str!("../../../../contracts/extraction/v1/fixtures/mining-states.json");
const EXTRACTION_STATES_JSON: &str =
    include_str!("../../../../contracts/extraction/v1/fixtures/extraction-states.json");
const COMBAT_STATES_JSON: &str =
    include_str!("../../../../contracts/extraction/v1/fixtures/combat-states.json");
const GET_STATE_RESULTS_JSON: &str =
    include_str!("../../../../contracts/extraction/v1/fixtures/get-state-results.json");

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

pub fn bundled_gameplay_intent_fixture() -> Result<EnvelopeFixture, ContractError> {
    serde_json::from_str(GAMEPLAY_INTENTS_JSON)
        .map_err(|error| ContractError::new(format!("gameplay intent fixture JSON 无效: {error}")))
}

pub fn bundled_mining_state_fixture() -> Result<EnvelopeFixture, ContractError> {
    serde_json::from_str(MINING_STATES_JSON)
        .map_err(|error| ContractError::new(format!("mining state fixture JSON 无效: {error}")))
}

pub fn bundled_extraction_state_fixture() -> Result<EnvelopeFixture, ContractError> {
    serde_json::from_str(EXTRACTION_STATES_JSON)
        .map_err(|error| ContractError::new(format!("extraction state fixture JSON 无效: {error}")))
}

pub fn bundled_combat_state_fixture() -> Result<EnvelopeFixture, ContractError> {
    serde_json::from_str(COMBAT_STATES_JSON)
        .map_err(|error| ContractError::new(format!("combat state fixture JSON 无效: {error}")))
}

pub fn bundled_get_state_fixture() -> Result<EnvelopeFixture, ContractError> {
    serde_json::from_str(GET_STATE_RESULTS_JSON)
        .map_err(|error| ContractError::new(format!("get-state fixture JSON 无效: {error}")))
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
