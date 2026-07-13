import {
  assertOnlyKeys,
  readBoolean,
  readEnum,
  readNonEmptyString,
  readRecord,
  readUuid,
} from "../../../../contracts/extraction/v1/decoder-utils";

import type {
  MatchResult,
  QueueSnapshot,
  ResourceCounts,
  WarehouseSnapshot,
} from "./models";

const QUEUE_STATUSES = [
  "idle",
  "queued",
  "preparing",
  "active",
  "extractionOpen",
  "settling",
] as const;
const RESULT_STATUSES = [
  "pendingReconciliation",
  "extracted",
  "dead",
  "timedOut",
  "aborted",
] as const;
const TERMINAL_CAUSES = ["melee", "reconnectTimeout", "hardDeadline"] as const;
const RFC3339_PATTERN =
  /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d{1,9})?(?:Z|[+-]\d{2}:\d{2})$/u;

export function decodeQueueSnapshot(value: unknown): QueueSnapshot {
  const source = readRecord(value, "queueResponse");
  assertOnlyKeys(
    source,
    ["status", "position", "enqueuedAt", "matchId", "worldName", "removed"],
    "queueResponse",
  );
  const snapshot: QueueSnapshot = {
    status: readEnum(source.status, QUEUE_STATUSES, "queueResponse.status"),
  };
  if (source.position !== undefined) {
    snapshot.position = readSafeNonNegative(
      source.position,
      "queueResponse.position",
    );
  }
  if (source.enqueuedAt !== undefined) {
    snapshot.enqueuedAt = readTimestamp(
      source.enqueuedAt,
      "queueResponse.enqueuedAt",
    );
  }
  if (source.matchId !== undefined) {
    snapshot.matchId = readUuid(source.matchId, "queueResponse.matchId");
  }
  if (source.worldName !== undefined) {
    snapshot.worldName = readNonEmptyString(
      source.worldName,
      "queueResponse.worldName",
    );
  }
  if (source.removed !== undefined) {
    snapshot.removed = readBoolean(source.removed, "queueResponse.removed");
  }
  if ((snapshot.matchId === undefined) !== (snapshot.worldName === undefined)) {
    throw new Error(
      "queueResponse: matchId and worldName must appear together",
    );
  }
  return snapshot;
}

export function decodeWarehouse(value: unknown): WarehouseSnapshot {
  const source = readRecord(value, "warehouseResponse");
  assertOnlyKeys(source, ["resources", "stats"], "warehouseResponse");
  const stats = readRecord(source.stats, "warehouseResponse.stats");
  assertOnlyKeys(
    stats,
    [
      "totalResourcesExtracted",
      "totalExtractionValue",
      "successfulExtractions",
      "highestSingleMatchValue",
    ],
    "warehouseResponse.stats",
  );
  return {
    resources: decodeResources(source.resources, "warehouseResponse.resources"),
    stats: {
      totalResourcesExtracted: readSafeNonNegative(
        stats.totalResourcesExtracted,
        "warehouseResponse.stats.totalResourcesExtracted",
      ),
      totalExtractionValue: readSafeNonNegative(
        stats.totalExtractionValue,
        "warehouseResponse.stats.totalExtractionValue",
      ),
      successfulExtractions: readSafeNonNegative(
        stats.successfulExtractions,
        "warehouseResponse.stats.successfulExtractions",
      ),
      highestSingleMatchValue: readSafeNonNegative(
        stats.highestSingleMatchValue,
        "warehouseResponse.stats.highestSingleMatchValue",
      ),
    },
  };
}

export function decodeOptionalMatchResult(value: unknown): MatchResult | null {
  return value === null ? null : decodeMatchResult(value);
}

export function decodeMatchResult(value: unknown): MatchResult {
  const source = readRecord(value, "matchResult");
  assertOnlyKeys(
    source,
    [
      "matchId",
      "status",
      "publicPlayerId",
      "terminalCause",
      "killerPublicPlayerId",
      "terminalAt",
      "survivedMs",
      "stats",
      "settlement",
    ],
    "matchResult",
  );
  const status = readEnum(source.status, RESULT_STATUSES, "matchResult.status");
  const result: MatchResult = {
    matchId: readUuid(source.matchId, "matchResult.matchId"),
    status,
    publicPlayerId: readUuid(
      source.publicPlayerId,
      "matchResult.publicPlayerId",
    ),
    terminalCause: readNullableCause(source.terminalCause),
    killerPublicPlayerId: readNullableUuid(
      source.killerPublicPlayerId,
      "matchResult.killerPublicPlayerId",
    ),
    terminalAt: readNullableTimestamp(
      source.terminalAt,
      "matchResult.terminalAt",
    ),
    survivedMs: readNullableSafeInteger(
      source.survivedMs,
      "matchResult.survivedMs",
    ),
    stats: decodeStats(source.stats),
    settlement: decodeSettlement(source.settlement),
  };
  validateResultShape(result);
  return result;
}

function decodeStats(value: unknown): MatchResult["stats"] {
  const source = readRecord(value, "matchResult.stats");
  assertOnlyKeys(source, ["mined", "pickedUp", "lost"], "matchResult.stats");
  return {
    mined: decodeResources(source.mined, "matchResult.stats.mined"),
    pickedUp: decodeResources(source.pickedUp, "matchResult.stats.pickedUp"),
    lost: decodeResources(source.lost, "matchResult.stats.lost"),
  };
}

function decodeSettlement(value: unknown): MatchResult["settlement"] {
  if (value === null) {
    return null;
  }
  const source = readRecord(value, "matchResult.settlement");
  assertOnlyKeys(
    source,
    ["settlementId", "resources", "totalValue", "configVersion", "committedAt"],
    "matchResult.settlement",
  );
  return {
    settlementId: readUuid(
      source.settlementId,
      "matchResult.settlement.settlementId",
    ),
    resources: decodeResources(
      source.resources,
      "matchResult.settlement.resources",
    ),
    totalValue: readSafeNonNegative(
      source.totalValue,
      "matchResult.settlement.totalValue",
    ),
    configVersion: readNonEmptyString(
      source.configVersion,
      "matchResult.settlement.configVersion",
    ),
    committedAt: readTimestamp(
      source.committedAt,
      "matchResult.settlement.committedAt",
    ),
  };
}

function decodeResources(value: unknown, path: string): ResourceCounts {
  const source = readRecord(value, path);
  assertOnlyKeys(source, ["dirt", "gold", "diamond"], path);
  return {
    dirt: readSafeNonNegative(source.dirt, `${path}.dirt`),
    gold: readSafeNonNegative(source.gold, `${path}.gold`),
    diamond: readSafeNonNegative(source.diamond, `${path}.diamond`),
  };
}

function validateResultShape(result: MatchResult): void {
  const terminal = result.terminalAt !== null && result.survivedMs !== null;
  const valid =
    (result.status === "extracted" &&
      result.settlement !== null &&
      !terminal &&
      result.terminalCause === null &&
      result.killerPublicPlayerId === null) ||
    (result.status === "dead" &&
      result.settlement === null &&
      terminal &&
      result.terminalCause === "melee" &&
      result.killerPublicPlayerId !== null) ||
    (result.status === "timedOut" &&
      result.settlement === null &&
      terminal &&
      (result.terminalCause === "reconnectTimeout" ||
        result.terminalCause === "hardDeadline") &&
      result.killerPublicPlayerId === null) ||
    ((result.status === "pendingReconciliation" ||
      result.status === "aborted") &&
      result.settlement === null &&
      !terminal &&
      result.terminalCause === null &&
      result.killerPublicPlayerId === null);
  if (!valid) {
    throw new Error("matchResult: inconsistent terminal shape");
  }
}

function readNullableCause(value: unknown): MatchResult["terminalCause"] {
  return value === null
    ? null
    : readEnum(value, TERMINAL_CAUSES, "matchResult.terminalCause");
}

function readNullableUuid(value: unknown, path: string): string | null {
  return value === null ? null : readUuid(value, path);
}

function readNullableTimestamp(value: unknown, path: string): string | null {
  return value === null ? null : readTimestamp(value, path);
}

function readNullableSafeInteger(value: unknown, path: string): number | null {
  return value === null ? null : readSafeNonNegative(value, path);
}

function readTimestamp(value: unknown, path: string): string {
  const timestamp = readNonEmptyString(value, path);
  if (
    !RFC3339_PATTERN.test(timestamp) ||
    !Number.isFinite(Date.parse(timestamp))
  ) {
    throw new Error(`${path}: expected RFC3339 timestamp`);
  }
  return timestamp;
}

function readSafeNonNegative(value: unknown, path: string): number {
  if (!Number.isSafeInteger(value) || (value as number) < 0) {
    throw new Error(`${path}: expected non-negative safe integer`);
  }
  return value as number;
}
