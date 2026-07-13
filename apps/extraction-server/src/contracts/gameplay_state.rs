use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::{
    ContractError, DeathResultEnvelope, EquipmentKey, ExtractionManifest, ExtractionStateEnvelope,
    HealthStateEnvelope, Intent, MiningStateEnvelope, ProtocolEnvelope, ResourceKey,
};
use crate::match_world::RESOURCE_BACKPACK_SLOTS;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GetStatePayload {}

pub fn decode_get_state_intent(
    envelope: &ProtocolEnvelope,
) -> Result<Intent<GetStatePayload>, ContractError> {
    envelope.decode_intent::<GetStatePayload>()
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceStackState {
    pub resource: ResourceKey,
    pub quantity: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InventoryState {
    pub slots: [Option<ResourceStackState>; RESOURCE_BACKPACK_SLOTS],
    pub revision: u32,
    pub frozen: bool,
    #[serde(deserialize_with = "deserialize_required_nullable_u32")]
    pub last_drop_sequence: Option<u32>,
}

fn deserialize_required_nullable_u32<'de, D>(deserializer: D) -> Result<Option<u32>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<u32>::deserialize(deserializer)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixedEquipmentState {
    pub pickaxe: EquipmentKey,
    pub melee_weapon: EquipmentKey,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InventoryStateData {
    pub inventory: InventoryState,
    pub equipment: FixedEquipmentState,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum InventoryStateKind {
    State,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
enum InventoryStateStream {
    Inventory,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InventoryStateEnvelope {
    protocol_version: u32,
    #[serde(rename = "type")]
    kind: InventoryStateKind,
    pub match_id: Uuid,
    stream: InventoryStateStream,
    pub revision: u32,
    pub data: InventoryStateData,
}

impl InventoryStateEnvelope {
    pub fn new(
        manifest: &ExtractionManifest,
        match_id: Uuid,
        revision: u32,
        data: InventoryStateData,
    ) -> Result<Self, ContractError> {
        let state = Self {
            protocol_version: manifest.protocol_version,
            kind: InventoryStateKind::State,
            match_id,
            stream: InventoryStateStream::Inventory,
            revision,
            data,
        };
        state.validate(manifest)?;
        Ok(state)
    }

    fn validate(&self, manifest: &ExtractionManifest) -> Result<(), ContractError> {
        if self.protocol_version != manifest.protocol_version
            || self.match_id.is_nil()
            || self.revision != self.data.inventory.revision
        {
            return Err(ContractError::new(
                "inventory state envelope 身份、版本或 revision 无效",
            ));
        }
        validate_inventory_assets(&self.data.inventory, self.data.equipment, manifest)
    }
}

pub fn decode_inventory_state(
    value: Value,
    manifest: &ExtractionManifest,
) -> Result<InventoryStateEnvelope, ContractError> {
    let state = serde_json::from_value::<InventoryStateEnvelope>(value)
        .map_err(|error| ContractError::new(format!("inventory state JSON 无效: {error}")))?;
    state.validate(manifest)?;
    Ok(state)
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttackCursorState {
    pub revision: u32,
    pub accepted_sequence: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GameplayStateData {
    pub match_id: Uuid,
    pub inventory: InventoryState,
    pub equipment: FixedEquipmentState,
    pub mining: MiningStateEnvelope,
    pub extraction: ExtractionStateEnvelope,
    pub health: HealthStateEnvelope,
    pub attack: AttackCursorState,
    pub death_result: Option<DeathResultEnvelope>,
}

impl GameplayStateData {
    fn validate(&self, manifest: &ExtractionManifest) -> Result<(), ContractError> {
        if self.match_id.is_nil()
            || self.mining.match_id != self.match_id
            || self.extraction.match_id != self.match_id
            || self.health.match_id != self.match_id
            || self
                .death_result
                .as_ref()
                .is_some_and(|result| result.match_id != self.match_id)
        {
            return Err(ContractError::new("get-state 内部 matchId 不一致"));
        }
        self.mining.validate(manifest)?;
        self.extraction.validate(manifest)?;
        self.health.validate(manifest)?;
        if let Some(result) = &self.death_result {
            result.validate(manifest)?;
        }
        validate_inventory_assets(&self.inventory, self.equipment, manifest)?;

        // 死亡是终态：快照必须同时冻结并清空临时背包，避免重连后复活或复制物资。
        if self.health.is_dead() {
            let Some(result) = &self.death_result else {
                return Err(ContractError::new("死亡 get-state 缺少 deathResult"));
            };
            if !self.inventory.frozen
                || self.inventory.slots.iter().any(Option::is_some)
                || result.revision != self.health.revision
                || !self.extraction.data.is_closed()
            {
                return Err(ContractError::new("死亡 get-state 终态不一致"));
            }
        } else if self.death_result.is_some()
            || self.inventory.frozen != self.extraction.data.is_pending()
        {
            return Err(ContractError::new("存活 get-state 撤离冻结状态不一致"));
        }
        Ok(())
    }
}

fn validate_inventory_assets(
    inventory: &InventoryState,
    equipment: FixedEquipmentState,
    manifest: &ExtractionManifest,
) -> Result<(), ContractError> {
    if equipment.pickaxe != EquipmentKey::BasicPickaxe
        || equipment.melee_weapon != EquipmentKey::BasicMeleeWeapon
    {
        return Err(ContractError::new("inventory state 固定装备无效"));
    }
    if inventory.slots.iter().flatten().any(|stack| {
        let max_stack = manifest
            .resources
            .iter()
            .find(|resource| resource.key == stack.resource)
            .map(|resource| resource.max_stack);
        stack.quantity == 0 || max_stack.is_none_or(|limit| stack.quantity > limit)
    }) {
        return Err(ContractError::new("inventory state 背包堆叠数量无效"));
    }
    Ok(())
}

pub fn decode_gameplay_state(
    value: Value,
    manifest: &ExtractionManifest,
) -> Result<GameplayStateData, ContractError> {
    let state = serde_json::from_value::<GameplayStateData>(value)
        .map_err(|error| ContractError::new(format!("get-state data JSON 无效: {error}")))?;
    state.validate(manifest)?;
    Ok(state)
}
