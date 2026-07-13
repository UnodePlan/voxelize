import {
  assertOnlyKeys,
  readArray,
  readEnum,
  readNonEmptyString,
  readRecord,
  readSignedInteger,
  readUnsignedInteger,
  readUuid,
} from "./decoder-utils";
import type {
  DecodedDropSlotIntent,
  DecodedMiningIntent,
  DropSlotPayload,
  ExtractionManifest,
  MiningPayload,
  MiningStateData,
  MiningStateEnvelope,
  ProtocolEnvelope,
} from "./types";
import { MINING_ACTIONS, MINING_IDLE_REASONS, RESOURCE_KEYS } from "./types";

const RESOURCE_BACKPACK_SLOTS = 12;

export function decodeDropSlotIntent(
  envelope: ProtocolEnvelope,
): DecodedDropSlotIntent {
  if (envelope.type !== "intent") {
    throw new Error("drop-slot: expected intent envelope");
  }
  const payload = envelope.payload;
  assertOnlyKeys(
    payload,
    ["slot", "expectedInventoryRevision"],
    "drop-slot.payload",
  );
  const decoded: DropSlotPayload = {
    slot: readUnsignedInteger(payload.slot, "drop-slot.payload.slot"),
    expectedInventoryRevision: readUnsignedInteger(
      payload.expectedInventoryRevision,
      "drop-slot.payload.expectedInventoryRevision",
    ),
  };
  if (decoded.slot >= RESOURCE_BACKPACK_SLOTS) {
    throw new Error("drop-slot.payload.slot: out of range");
  }
  return {
    requestId: envelope.requestId,
    sequence: envelope.sequence,
    payload: decoded,
  };
}

export function decodeMiningIntent(
  envelope: ProtocolEnvelope,
): DecodedMiningIntent {
  if (envelope.type !== "intent") {
    throw new Error("mining: expected intent envelope");
  }
  const payload = envelope.payload;
  const action = readEnum(payload.action, MINING_ACTIONS, "mining.action");
  let decoded: MiningPayload;
  if (action === "start") {
    assertOnlyKeys(payload, ["action", "voxel"], "mining.payload");
    decoded = {
      action,
      voxel: readVoxel(payload.voxel, "mining.payload.voxel"),
    };
  } else {
    assertOnlyKeys(payload, ["action"], "mining.payload");
    decoded = { action };
  }
  return {
    requestId: envelope.requestId,
    sequence: envelope.sequence,
    payload: decoded,
  };
}

export function decodeMiningStateEnvelope(
  value: unknown,
  manifest: ExtractionManifest,
): MiningStateEnvelope {
  const source = readRecord(value, "miningState");
  assertOnlyKeys(
    source,
    ["protocolVersion", "type", "matchId", "stream", "revision", "data"],
    "miningState",
  );
  if (
    readUnsignedInteger(
      source.protocolVersion,
      "miningState.protocolVersion",
    ) !== manifest.protocolVersion
  ) {
    throw new Error("miningState.protocolVersion: unsupported version");
  }
  if (readNonEmptyString(source.type, "miningState.type") !== "state") {
    throw new Error("miningState.type: expected state");
  }
  if (readNonEmptyString(source.stream, "miningState.stream") !== "mining") {
    throw new Error("miningState.stream: expected mining");
  }
  return {
    protocolVersion: manifest.protocolVersion,
    type: "state",
    matchId: readUuid(source.matchId, "miningState.matchId"),
    stream: "mining",
    revision: readUnsignedInteger(source.revision, "miningState.revision"),
    data: decodeMiningStateData(source.data),
  };
}

function decodeMiningStateData(value: unknown): MiningStateData {
  const source = readRecord(value, "miningState.data");
  const status = readNonEmptyString(source.status, "miningState.data.status");
  if (status === "idle") {
    assertOnlyKeys(
      source,
      ["status", "acceptedSequence", "reason"],
      "miningState.data",
    );
    return {
      status,
      acceptedSequence:
        source.acceptedSequence === null
          ? null
          : readUnsignedInteger(
              source.acceptedSequence,
              "miningState.data.acceptedSequence",
            ),
      reason: readEnum(
        source.reason,
        MINING_IDLE_REASONS,
        "miningState.data.reason",
      ),
    };
  }
  if (status === "mining") {
    assertOnlyKeys(
      source,
      [
        "status",
        "acceptedSequence",
        "target",
        "resource",
        "elapsedMs",
        "requiredMs",
      ],
      "miningState.data",
    );
    const requiredMs = readUnsignedInteger(
      source.requiredMs,
      "miningState.data.requiredMs",
    );
    const elapsedMs = readUnsignedInteger(
      source.elapsedMs,
      "miningState.data.elapsedMs",
    );
    if (requiredMs === 0 || elapsedMs > requiredMs) {
      throw new Error("miningState.data: invalid progress range");
    }
    return {
      status,
      acceptedSequence: readUnsignedInteger(
        source.acceptedSequence,
        "miningState.data.acceptedSequence",
      ),
      target: readVoxel(source.target, "miningState.data.target"),
      resource: readEnum(
        source.resource,
        RESOURCE_KEYS,
        "miningState.data.resource",
      ),
      elapsedMs,
      requiredMs,
    };
  }
  throw new Error(`miningState.data.status: unsupported value ${status}`);
}

function readVoxel(value: unknown, path: string): [number, number, number] {
  const coordinates = readArray(value, path);
  if (coordinates.length !== 3) {
    throw new Error(`${path}: expected three coordinates`);
  }
  return [
    readSignedInteger(coordinates[0], `${path}[0]`),
    readSignedInteger(coordinates[1], `${path}[1]`),
    readSignedInteger(coordinates[2], `${path}[2]`),
  ];
}
