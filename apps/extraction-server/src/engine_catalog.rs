use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};
use voxelize::{Block, ItemComponentName, ItemRegistry, Registry};

use crate::{
    contracts::{EquipmentKey, ExtractionManifest, ResourceKey},
    generation::GenerationConfig,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeResource {
    pub voxel_id: u32,
    pub item_id: u32,
    pub max_stack: u32,
    pub score_weight: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RuntimeEquipment {
    pub item_id: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StackableItem {
    pub max_stack: u32,
}

impl ItemComponentName for StackableItem {
    const COMPONENT_NAME: &'static str = "stackable";
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct MatchResourceCatalog {
    pub catalog_version: u32,
    pub dirt: RuntimeResource,
    pub gold: RuntimeResource,
    pub diamond: RuntimeResource,
    pub basic_pickaxe: RuntimeEquipment,
    pub basic_melee_weapon: RuntimeEquipment,
}

#[derive(Clone)]
pub(crate) struct EngineCatalog {
    blocks: Registry,
    items: ItemRegistry,
    resources: MatchResourceCatalog,
}

impl EngineCatalog {
    pub(crate) fn from_manifest(manifest: &ExtractionManifest) -> Result<Self, CatalogError> {
        manifest
            .validate()
            .map_err(|error| CatalogError::new(error.to_string()))?;
        if manifest.catalog_version != 1
            || GenerationConfig::resolve(&manifest.generation_version, &manifest.config_version)
                .is_none()
        {
            return Err(CatalogError::new("不支持的资源或地图配置版本"));
        }

        let resources = MatchResourceCatalog {
            catalog_version: manifest.catalog_version,
            dirt: runtime_resource(manifest, ResourceKey::Dirt)?,
            gold: runtime_resource(manifest, ResourceKey::Gold)?,
            diamond: runtime_resource(manifest, ResourceKey::Diamond)?,
            basic_pickaxe: runtime_equipment(manifest, EquipmentKey::BasicPickaxe)?,
            basic_melee_weapon: runtime_equipment(manifest, EquipmentKey::BasicMeleeWeapon)?,
        };
        let mut blocks = Registry::new();
        for key in ResourceKey::ALL {
            let definition = resource_definition(manifest, key)?;
            blocks.register_block(
                &Block::new(definition.key.as_str())
                    .id(definition.voxel_id)
                    .build(),
            );
        }
        let mut items = ItemRegistry::new();
        for key in ResourceKey::ALL {
            let definition = resource_definition(manifest, key)?;
            items.register_with_id(definition.item_id, definition.key.as_str(), |builder| {
                builder.with(StackableItem {
                    max_stack: definition.max_stack,
                })
            });
        }
        for key in EquipmentKey::ALL {
            let definition = equipment_definition(manifest, key)?;
            items.register_with_id(definition.item_id, definition.key.as_str(), |builder| {
                builder
            });
        }

        Ok(Self {
            blocks,
            items,
            resources,
        })
    }

    pub(crate) fn blocks(&self) -> &Registry {
        &self.blocks
    }

    pub(crate) fn items(&self) -> &ItemRegistry {
        &self.items
    }

    pub(crate) fn resources(&self) -> MatchResourceCatalog {
        self.resources
    }
}

fn runtime_resource(
    manifest: &ExtractionManifest,
    key: ResourceKey,
) -> Result<RuntimeResource, CatalogError> {
    resource_definition(manifest, key).map(|definition| RuntimeResource {
        voxel_id: definition.voxel_id,
        item_id: definition.item_id,
        max_stack: definition.max_stack,
        score_weight: definition.score_weight,
    })
}

fn runtime_equipment(
    manifest: &ExtractionManifest,
    key: EquipmentKey,
) -> Result<RuntimeEquipment, CatalogError> {
    equipment_definition(manifest, key).map(|definition| RuntimeEquipment {
        item_id: definition.item_id,
    })
}

fn resource_definition(
    manifest: &ExtractionManifest,
    key: ResourceKey,
) -> Result<&crate::contracts::ResourceDefinition, CatalogError> {
    manifest
        .resources
        .iter()
        .find(|definition| definition.key == key)
        .ok_or_else(|| CatalogError::new(format!("缺少资源定义: {}", key.as_str())))
}

fn equipment_definition(
    manifest: &ExtractionManifest,
    key: EquipmentKey,
) -> Result<&crate::contracts::EquipmentDefinition, CatalogError> {
    manifest
        .equipment
        .iter()
        .find(|definition| definition.key == key)
        .ok_or_else(|| CatalogError::new(format!("缺少装备定义: {}", key.as_str())))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CatalogError {
    message: String,
}

impl CatalogError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for CatalogError {}

#[cfg(test)]
mod tests;
