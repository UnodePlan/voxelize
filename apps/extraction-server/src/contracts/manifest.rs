use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use super::ContractError;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExtractionManifest {
    pub protocol_version: u32,
    pub catalog_version: u32,
    pub gameplay_version: String,
    pub generation_version: String,
    pub config_version: String,
    pub resources: Vec<ResourceDefinition>,
    pub equipment: Vec<EquipmentDefinition>,
    pub error_codes: Vec<ErrorCode>,
}

impl ExtractionManifest {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.protocol_version != 1 || self.catalog_version == 0 {
            return Err(ContractError::new("manifest 版本不受支持"));
        }
        if self.gameplay_version.is_empty()
            || self.generation_version.is_empty()
            || self.config_version.is_empty()
        {
            return Err(ContractError::new("manifest 版本名称不能为空"));
        }

        let resource_keys: HashSet<_> = self.resources.iter().map(|item| item.key).collect();
        let expected_resources =
            HashSet::from([ResourceKey::Dirt, ResourceKey::Gold, ResourceKey::Diamond]);
        if resource_keys != expected_resources || resource_keys.len() != self.resources.len() {
            return Err(ContractError::new("资源键必须完整且唯一"));
        }

        let equipment_keys: HashSet<_> = self.equipment.iter().map(|item| item.key).collect();
        let expected_equipment =
            HashSet::from([EquipmentKey::BasicPickaxe, EquipmentKey::BasicMeleeWeapon]);
        if equipment_keys != expected_equipment || equipment_keys.len() != self.equipment.len() {
            return Err(ContractError::new("装备键必须完整且唯一"));
        }

        self.validate_ids()?;
        self.validate_errors()
    }

    fn validate_ids(&self) -> Result<(), ContractError> {
        let mut voxel_ids = HashSet::new();
        let mut item_ids = HashSet::new();

        for resource in &self.resources {
            if resource.voxel_id == 0
                || resource.voxel_id > u16::MAX as u32
                || !voxel_ids.insert(resource.voxel_id)
            {
                return Err(ContractError::new("资源 voxelId 必须在 1..=65535 且唯一"));
            }
            if resource.max_stack != 64 || resource.score_weight == 0 {
                return Err(ContractError::new("资源堆叠或统计权重无效"));
            }
            if resource.item_id == 0 || !item_ids.insert(resource.item_id) {
                return Err(ContractError::new("资源 itemId 必须非零且唯一"));
            }
        }

        for equipment in &self.equipment {
            if equipment.item_id == 0 || !item_ids.insert(equipment.item_id) {
                return Err(ContractError::new("装备 itemId 必须非零且全局唯一"));
            }
        }
        Ok(())
    }

    fn validate_errors(&self) -> Result<(), ContractError> {
        let error_codes: HashSet<_> = self.error_codes.iter().copied().collect();
        let expected: HashSet<_> = ErrorCode::ALL.into_iter().collect();
        if error_codes != expected || error_codes.len() != self.error_codes.len() {
            return Err(ContractError::new("错误码必须完整且唯一"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKey {
    Dirt,
    Gold,
    Diamond,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceDefinition {
    pub key: ResourceKey,
    pub voxel_id: u32,
    pub item_id: u32,
    pub max_stack: u32,
    pub score_weight: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EquipmentKey {
    BasicPickaxe,
    BasicMeleeWeapon,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EquipmentDefinition {
    pub key: EquipmentKey,
    pub item_id: u32,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    ProtocolUnsupportedVersion,
    RequestMalformed,
    AuthRequired,
    AuthWrongNetwork,
    AuthInvalidSiwe,
    AuthNonceInvalid,
    MatchFull,
    MatchRosterLocked,
    MatchReconnectExpired,
    GameInvalidState,
    GameStaleSequence,
    GameStaleRevision,
    GameOutOfRange,
    GameCooldown,
    InventorySlotInvalid,
    SettlementPending,
    SettlementConflict,
    ServiceUnavailable,
}

impl ErrorCode {
    const ALL: [Self; 18] = [
        Self::ProtocolUnsupportedVersion,
        Self::RequestMalformed,
        Self::AuthRequired,
        Self::AuthWrongNetwork,
        Self::AuthInvalidSiwe,
        Self::AuthNonceInvalid,
        Self::MatchFull,
        Self::MatchRosterLocked,
        Self::MatchReconnectExpired,
        Self::GameInvalidState,
        Self::GameStaleSequence,
        Self::GameStaleRevision,
        Self::GameOutOfRange,
        Self::GameCooldown,
        Self::InventorySlotInvalid,
        Self::SettlementPending,
        Self::SettlementConflict,
        Self::ServiceUnavailable,
    ];
}
