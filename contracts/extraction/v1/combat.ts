import {
  ATTACK_RESOLUTIONS,
  ATTACK_WEAPON_SLOTS,
  DEATH_CAUSES,
} from "./combat-types";
import type {
  AttackResultData,
  DecodedAttackIntent,
  DeathResultData,
  DeathResultEnvelope,
  HealthStateData,
  HealthStateEnvelope,
  ResourceTally,
} from "./combat-types";
import {
  assertOnlyKeys,
  readEnum,
  readNonEmptyString,
  readRecord,
  readUnsignedInteger,
  readUuid,
} from "./decoder-utils";
import type { ExtractionManifest, ProtocolEnvelope } from "./types";

const MAX_HALF_HEARTS = 20;

export function decodeAttackIntent(
  envelope: ProtocolEnvelope,
): DecodedAttackIntent {
  if (envelope.type !== "intent") {
    throw new Error("attack: expected intent envelope");
  }
  assertOnlyKeys(envelope.payload, ["weaponSlot"], "attack.payload");
  return {
    requestId: envelope.requestId,
    sequence: envelope.sequence,
    payload: {
      weaponSlot: readEnum(
        envelope.payload.weaponSlot,
        ATTACK_WEAPON_SLOTS,
        "attack.payload.weaponSlot",
      ),
    },
  };
}

export function decodeAttackResultData(value: unknown): AttackResultData {
  const source = readRecord(value, "attackResult.data");
  assertOnlyKeys(
    source,
    ["acceptedSequence", "attackRevision", "resolution"],
    "attackResult.data",
  );
  return {
    acceptedSequence: readUnsignedInteger(
      source.acceptedSequence,
      "attackResult.data.acceptedSequence",
    ),
    attackRevision: readUnsignedInteger(
      source.attackRevision,
      "attackResult.data.attackRevision",
    ),
    resolution: readEnum(
      source.resolution,
      ATTACK_RESOLUTIONS,
      "attackResult.data.resolution",
    ),
  };
}

export function decodeHealthStateEnvelope(
  value: unknown,
  manifest: ExtractionManifest,
): HealthStateEnvelope {
  const source = readStateEnvelope(value, manifest, "healthState", "health");
  return {
    protocolVersion: manifest.protocolVersion,
    type: "state",
    matchId: source.matchId,
    stream: "health",
    revision: source.revision,
    data: decodeHealthStateData(source.data),
  };
}

export function decodeDeathResultEnvelope(
  value: unknown,
  manifest: ExtractionManifest,
): DeathResultEnvelope {
  const source = readStateEnvelope(
    value,
    manifest,
    "deathResult",
    "deathResult",
  );
  const data = decodeDeathResultData(source.data);
  const killerIsValid =
    (data.cause === "melee" && data.killerPublicPlayerId !== null) ||
    ((data.cause === "reconnectTimeout" || data.cause === "hardDeadline") &&
      data.killerPublicPlayerId === null);
  if (!killerIsValid) {
    throw new Error("deathResult.data: killer does not match cause");
  }
  return {
    protocolVersion: manifest.protocolVersion,
    type: "state",
    matchId: source.matchId,
    stream: "deathResult",
    revision: source.revision,
    data,
  };
}

function decodeHealthStateData(value: unknown): HealthStateData {
  const source = readRecord(value, "healthState.data");
  assertOnlyKeys(
    source,
    ["status", "currentHalfHearts", "maxHalfHearts"],
    "healthState.data",
  );
  const status = readNonEmptyString(source.status, "healthState.data.status");
  const currentHalfHearts = readUnsignedInteger(
    source.currentHalfHearts,
    "healthState.data.currentHalfHearts",
  );
  const maxHalfHearts = readUnsignedInteger(
    source.maxHalfHearts,
    "healthState.data.maxHalfHearts",
  );
  if (maxHalfHearts !== MAX_HALF_HEARTS) {
    throw new Error("healthState.data.maxHalfHearts: expected 20");
  }
  if (status === "alive" && currentHalfHearts >= 1 && currentHalfHearts <= 20) {
    return { status, currentHalfHearts, maxHalfHearts: 20 };
  }
  if (status === "dead" && currentHalfHearts === 0) {
    return { status, currentHalfHearts: 0, maxHalfHearts: 20 };
  }
  throw new Error("healthState.data: invalid status or half-heart count");
}

function decodeDeathResultData(value: unknown): DeathResultData {
  const source = readRecord(value, "deathResult.data");
  assertOnlyKeys(
    source,
    [
      "cause",
      "killerPublicPlayerId",
      "survivedMs",
      "mined",
      "pickedUp",
      "lost",
    ],
    "deathResult.data",
  );
  return {
    cause: readEnum(source.cause, DEATH_CAUSES, "deathResult.data.cause"),
    killerPublicPlayerId:
      source.killerPublicPlayerId === null
        ? null
        : readUuid(
            source.killerPublicPlayerId,
            "deathResult.data.killerPublicPlayerId",
          ),
    survivedMs: readUnsignedInteger(
      source.survivedMs,
      "deathResult.data.survivedMs",
    ),
    mined: decodeResourceTally(source.mined, "deathResult.data.mined"),
    pickedUp: decodeResourceTally(source.pickedUp, "deathResult.data.pickedUp"),
    lost: decodeResourceTally(source.lost, "deathResult.data.lost"),
  };
}

function decodeResourceTally(value: unknown, path: string): ResourceTally {
  const source = readRecord(value, path);
  assertOnlyKeys(source, ["dirt", "gold", "diamond"], path);
  return {
    dirt: readUnsignedInteger(source.dirt, `${path}.dirt`),
    gold: readUnsignedInteger(source.gold, `${path}.gold`),
    diamond: readUnsignedInteger(source.diamond, `${path}.diamond`),
  };
}

function readStateEnvelope(
  value: unknown,
  manifest: ExtractionManifest,
  path: string,
  expectedStream: "health" | "deathResult",
): { matchId: string; revision: number; data: unknown } {
  const source = readRecord(value, path);
  assertOnlyKeys(
    source,
    ["protocolVersion", "type", "matchId", "stream", "revision", "data"],
    path,
  );
  if (
    readUnsignedInteger(source.protocolVersion, `${path}.protocolVersion`) !==
    manifest.protocolVersion
  ) {
    throw new Error(`${path}.protocolVersion: unsupported version`);
  }
  if (readNonEmptyString(source.type, `${path}.type`) !== "state") {
    throw new Error(`${path}.type: expected state`);
  }
  if (readNonEmptyString(source.stream, `${path}.stream`) !== expectedStream) {
    throw new Error(`${path}.stream: expected ${expectedStream}`);
  }
  return {
    matchId: readUuid(source.matchId, `${path}.matchId`),
    revision: readUnsignedInteger(source.revision, `${path}.revision`),
    data: source.data,
  };
}
