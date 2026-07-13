import { decodeDeathResultEnvelope, decodeHealthStateEnvelope } from "./combat";
import {
  assertOnlyKeys,
  readArray,
  readBoolean,
  readEnum,
  readRecord,
  readUnsignedInteger,
  readUuid,
} from "./decoder-utils";
import { decodeExtractionStateEnvelope } from "./extraction";
import { decodeMiningStateEnvelope } from "./gameplay";
import type {
  AttackCursorState,
  FixedEquipmentState,
  GameplayStateData,
  InventoryState,
  InventoryStateData,
  InventoryStateEnvelope,
  ResourceStackState,
  DecodedGetStateIntent,
} from "./gameplay-state-types";
import type { ExtractionManifest, ProtocolEnvelope } from "./types";
import { EQUIPMENT_KEYS, RESOURCE_KEYS } from "./types";

const RESOURCE_BACKPACK_SLOTS = 12;

export function decodeGetStateIntent(
  envelope: ProtocolEnvelope,
): DecodedGetStateIntent {
  if (envelope.type !== "intent") {
    throw new Error("get-state: expected intent envelope");
  }
  assertOnlyKeys(envelope.payload, [], "get-state.payload");
  return {
    requestId: envelope.requestId,
    sequence: envelope.sequence,
    payload: {},
  };
}

export function decodeInventoryStateEnvelope(
  value: unknown,
  manifest: ExtractionManifest,
): InventoryStateEnvelope {
  const source = readRecord(value, "inventoryState");
  assertOnlyKeys(
    source,
    ["protocolVersion", "type", "matchId", "stream", "revision", "data"],
    "inventoryState",
  );
  const protocolVersion = readUnsignedInteger(
    source.protocolVersion,
    "inventoryState.protocolVersion",
  );
  if (protocolVersion !== manifest.protocolVersion || source.type !== "state") {
    throw new Error("inventoryState: incompatible envelope");
  }
  if (source.stream !== "inventory") {
    throw new Error("inventoryState: invalid stream");
  }
  const revision = readUnsignedInteger(
    source.revision,
    "inventoryState.revision",
  );
  const data = decodeInventoryStateData(source.data, manifest);
  if (revision !== data.inventory.revision) {
    throw new Error("inventoryState: envelope revision mismatch");
  }
  return {
    protocolVersion,
    type: "state",
    matchId: readUuid(source.matchId, "inventoryState.matchId"),
    stream: "inventory",
    revision,
    data,
  };
}

function decodeInventoryStateData(
  value: unknown,
  manifest: ExtractionManifest,
): InventoryStateData {
  const source = readRecord(value, "inventoryState.data");
  assertOnlyKeys(source, ["inventory", "equipment"], "inventoryState.data");
  return {
    inventory: decodeInventory(
      source.inventory,
      manifest,
      "inventoryState.data.inventory",
    ),
    equipment: decodeEquipment(
      source.equipment,
      "inventoryState.data.equipment",
    ),
  };
}

export function decodeGameplayStateData(
  value: unknown,
  manifest: ExtractionManifest,
): GameplayStateData {
  const source = readRecord(value, "gameplayState");
  assertOnlyKeys(
    source,
    [
      "matchId",
      "inventory",
      "equipment",
      "mining",
      "extraction",
      "health",
      "attack",
      "deathResult",
    ],
    "gameplayState",
  );
  const matchId = readUuid(source.matchId, "gameplayState.matchId");
  const inventory = decodeInventory(
    source.inventory,
    manifest,
    "gameplayState.inventory",
  );
  const equipment = decodeEquipment(
    source.equipment,
    "gameplayState.equipment",
  );
  const mining = decodeMiningStateEnvelope(source.mining, manifest);
  const extraction = decodeExtractionStateEnvelope(source.extraction, manifest);
  const health = decodeHealthStateEnvelope(source.health, manifest);
  const attack = decodeAttackCursor(source.attack);
  const deathResult =
    source.deathResult === null
      ? null
      : decodeDeathResultEnvelope(source.deathResult, manifest);

  if (
    mining.matchId !== matchId ||
    extraction.matchId !== matchId ||
    health.matchId !== matchId ||
    (deathResult !== null && deathResult.matchId !== matchId)
  ) {
    throw new Error("gameplayState: nested matchId mismatch");
  }

  // 死亡快照必须原子呈现终态，避免重连时复活或重复获得临时物资。
  if (health.data.status === "dead") {
    if (
      deathResult === null ||
      deathResult.revision !== health.revision ||
      !inventory.frozen ||
      inventory.slots.some((slot) => slot !== null) ||
      extraction.data.status !== "closed"
    ) {
      throw new Error("gameplayState: inconsistent dead terminal state");
    }
  } else if (
    deathResult !== null ||
    inventory.frozen !== (extraction.data.status === "pending")
  ) {
    throw new Error("gameplayState: inconsistent extraction freeze state");
  }

  return {
    matchId,
    inventory,
    equipment,
    mining,
    extraction,
    health,
    attack,
    deathResult,
  };
}

function decodeInventory(
  value: unknown,
  manifest: ExtractionManifest,
  path: string,
): InventoryState {
  const source = readRecord(value, path);
  assertOnlyKeys(
    source,
    ["slots", "revision", "frozen", "lastDropSequence"],
    path,
  );
  const slots = readArray(source.slots, `${path}.slots`);
  if (slots.length !== RESOURCE_BACKPACK_SLOTS) {
    throw new Error(`${path}.slots: expected 12 slots`);
  }
  return {
    slots: slots.map((slot, index) =>
      slot === null
        ? null
        : decodeResourceStack(slot, manifest, `${path}.slots[${index}]`),
    ),
    revision: readUnsignedInteger(source.revision, `${path}.revision`),
    frozen: readBoolean(source.frozen, `${path}.frozen`),
    lastDropSequence:
      source.lastDropSequence === null
        ? null
        : readUnsignedInteger(
            source.lastDropSequence,
            `${path}.lastDropSequence`,
          ),
  };
}

function decodeResourceStack(
  value: unknown,
  manifest: ExtractionManifest,
  path: string,
): ResourceStackState {
  const source = readRecord(value, path);
  assertOnlyKeys(source, ["resource", "quantity"], path);
  const resource = readEnum(source.resource, RESOURCE_KEYS, `${path}.resource`);
  const quantity = readUnsignedInteger(source.quantity, `${path}.quantity`);
  const maxStack = manifest.resources.find(
    (item) => item.key === resource,
  )?.maxStack;
  if (quantity === 0 || maxStack === undefined || quantity > maxStack) {
    throw new Error(`${path}.quantity: invalid stack quantity`);
  }
  return { resource, quantity };
}

function decodeEquipment(value: unknown, path: string): FixedEquipmentState {
  const source = readRecord(value, path);
  assertOnlyKeys(source, ["pickaxe", "meleeWeapon"], path);
  const pickaxe = readEnum(source.pickaxe, EQUIPMENT_KEYS, `${path}.pickaxe`);
  const meleeWeapon = readEnum(
    source.meleeWeapon,
    EQUIPMENT_KEYS,
    `${path}.meleeWeapon`,
  );
  if (pickaxe !== "basic_pickaxe" || meleeWeapon !== "basic_melee_weapon") {
    throw new Error(`${path}: invalid fixed equipment`);
  }
  return { pickaxe, meleeWeapon };
}

function decodeAttackCursor(value: unknown): AttackCursorState {
  const source = readRecord(value, "gameplayState.attack");
  assertOnlyKeys(
    source,
    ["revision", "acceptedSequence"],
    "gameplayState.attack",
  );
  return {
    revision: readUnsignedInteger(
      source.revision,
      "gameplayState.attack.revision",
    ),
    acceptedSequence:
      source.acceptedSequence === null
        ? null
        : readUnsignedInteger(
            source.acceptedSequence,
            "gameplayState.attack.acceptedSequence",
          ),
  };
}
