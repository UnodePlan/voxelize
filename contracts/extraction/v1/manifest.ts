import {
  assertExactSet,
  assertOnlyKeys,
  assertUnique,
  readArray,
  readEnum,
  readNonEmptyString,
  readPositiveInteger,
  readRecord,
  readUnsignedInteger,
} from "./decoder-utils";
import {
  EQUIPMENT_KEYS,
  ERROR_CODES,
  RESOURCE_KEYS,
  type EquipmentDefinition,
  type ExtractionManifest,
  type ResourceDefinition,
} from "./types";

const MAX_VOXEL_ID = 65_535;
const MAX_ITEM_ID = 2_147_483_647;

export function decodeExtractionManifest(value: unknown): ExtractionManifest {
  const source = readRecord(value, "manifest");
  assertOnlyKeys(
    source,
    [
      "protocolVersion",
      "catalogVersion",
      "gameplayVersion",
      "generationVersion",
      "configVersion",
      "resources",
      "equipment",
      "errorCodes",
    ],
    "manifest",
  );

  const manifest: ExtractionManifest = {
    protocolVersion: readUnsignedInteger(
      source.protocolVersion,
      "protocolVersion",
    ),
    catalogVersion: readPositiveInteger(
      source.catalogVersion,
      "catalogVersion",
    ),
    gameplayVersion: readNonEmptyString(
      source.gameplayVersion,
      "gameplayVersion",
    ),
    generationVersion: readNonEmptyString(
      source.generationVersion,
      "generationVersion",
    ),
    configVersion: readNonEmptyString(source.configVersion, "configVersion"),
    resources: readArray(source.resources, "resources").map(decodeResource),
    equipment: readArray(source.equipment, "equipment").map(decodeEquipment),
    errorCodes: readArray(source.errorCodes, "errorCodes").map((code, index) =>
      readEnum(code, ERROR_CODES, `errorCodes[${index}]`),
    ),
  };

  if (manifest.protocolVersion !== 1) {
    throw new Error("protocolVersion: unsupported version");
  }

  assertExactSet(
    manifest.resources.map(({ key }) => key),
    RESOURCE_KEYS,
    "resources",
  );
  assertExactSet(
    manifest.equipment.map(({ key }) => key),
    EQUIPMENT_KEYS,
    "equipment",
  );
  assertUnique(
    manifest.resources.map(({ voxelId }) => voxelId),
    "resources.voxelId",
  );
  assertUnique(
    [
      ...manifest.resources.map(({ itemId }) => itemId),
      ...manifest.equipment.map(({ itemId }) => itemId),
    ],
    "itemId",
  );
  assertExactSet(manifest.errorCodes, ERROR_CODES, "errorCodes");

  return manifest;
}

function decodeResource(value: unknown, index: number): ResourceDefinition {
  const source = readRecord(value, `resources[${index}]`);
  assertOnlyKeys(
    source,
    ["key", "voxelId", "itemId", "maxStack", "scoreWeight"],
    "resource",
  );
  const key = readEnum(source.key, RESOURCE_KEYS, "resource.key");
  const voxelId = readUnsignedInteger(source.voxelId, "resource.voxelId");
  const maxStack = readUnsignedInteger(source.maxStack, "resource.maxStack");

  if (voxelId === 0 || voxelId > MAX_VOXEL_ID) {
    throw new Error("resource.voxelId: outside 1..=65535");
  }
  if (maxStack !== 64) {
    throw new Error("resource.maxStack: expected 64");
  }

  return {
    key,
    voxelId,
    itemId: readItemId(source.itemId, "resource.itemId"),
    maxStack,
    scoreWeight: readPositiveInteger(
      source.scoreWeight,
      "resource.scoreWeight",
    ),
  };
}

function decodeEquipment(value: unknown, index: number): EquipmentDefinition {
  const source = readRecord(value, `equipment[${index}]`);
  assertOnlyKeys(source, ["key", "itemId"], "equipment");
  return {
    key: readEnum(source.key, EQUIPMENT_KEYS, "equipment.key"),
    itemId: readItemId(source.itemId, "equipment.itemId"),
  };
}

function readItemId(value: unknown, path: string): number {
  const itemId = readPositiveInteger(value, path);
  if (itemId > MAX_ITEM_ID) {
    throw new Error(`${path}: outside 1..=2147483647`);
  }
  return itemId;
}
