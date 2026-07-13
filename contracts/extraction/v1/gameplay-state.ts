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
import { decodeMiningStateEnvelope } from "./gameplay";
import type {
  AttackCursorState,
  FixedEquipmentState,
  GameplayStateData,
  InventoryState,
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
      "health",
      "attack",
      "deathResult",
    ],
    "gameplayState",
  );
  const matchId = readUuid(source.matchId, "gameplayState.matchId");
  const inventory = decodeInventory(source.inventory, manifest);
  const equipment = decodeEquipment(source.equipment);
  const mining = decodeMiningStateEnvelope(source.mining, manifest);
  const health = decodeHealthStateEnvelope(source.health, manifest);
  const attack = decodeAttackCursor(source.attack);
  const deathResult =
    source.deathResult === null
      ? null
      : decodeDeathResultEnvelope(source.deathResult, manifest);

  if (
    mining.matchId !== matchId ||
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
      inventory.slots.some((slot) => slot !== null)
    ) {
      throw new Error("gameplayState: inconsistent dead terminal state");
    }
  } else if (deathResult !== null || inventory.frozen) {
    throw new Error("gameplayState: alive state contains terminal data");
  }

  return {
    matchId,
    inventory,
    equipment,
    mining,
    health,
    attack,
    deathResult,
  };
}

function decodeInventory(
  value: unknown,
  manifest: ExtractionManifest,
): InventoryState {
  const source = readRecord(value, "gameplayState.inventory");
  assertOnlyKeys(
    source,
    ["slots", "revision", "frozen"],
    "gameplayState.inventory",
  );
  const slots = readArray(source.slots, "gameplayState.inventory.slots");
  if (slots.length !== RESOURCE_BACKPACK_SLOTS) {
    throw new Error("gameplayState.inventory.slots: expected 12 slots");
  }
  return {
    slots: slots.map((slot, index) =>
      slot === null
        ? null
        : decodeResourceStack(
            slot,
            manifest,
            `gameplayState.inventory.slots[${index}]`,
          ),
    ),
    revision: readUnsignedInteger(
      source.revision,
      "gameplayState.inventory.revision",
    ),
    frozen: readBoolean(source.frozen, "gameplayState.inventory.frozen"),
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

function decodeEquipment(value: unknown): FixedEquipmentState {
  const source = readRecord(value, "gameplayState.equipment");
  assertOnlyKeys(source, ["pickaxe", "meleeWeapon"], "gameplayState.equipment");
  const pickaxe = readEnum(
    source.pickaxe,
    EQUIPMENT_KEYS,
    "gameplayState.equipment.pickaxe",
  );
  const meleeWeapon = readEnum(
    source.meleeWeapon,
    EQUIPMENT_KEYS,
    "gameplayState.equipment.meleeWeapon",
  );
  if (pickaxe !== "basic_pickaxe" || meleeWeapon !== "basic_melee_weapon") {
    throw new Error("gameplayState.equipment: invalid fixed equipment");
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
