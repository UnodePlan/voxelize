import {
  assertOnlyKeys,
  readArray,
  readBoolean,
  readRecord,
  readUnsignedInteger,
} from "../../../../contracts/extraction/v1/decoder-utils";

import { LiveApiError, requestJson, type LiveHttpTransport } from "./http";

const SERVER_KEYS = [
  "worlds",
  "worldGenerations",
  "removingWorlds",
  "lostSessions",
  "transportSessions",
  "connections",
  "connectionPrincipals",
  "pendingJoins",
  "leavingSessions",
  "connectionClientIds",
  "connectionAttachAttemptIds",
  "detachedConnections",
  "pendingDetaches",
  "pendingRebinds",
  "pendingWorldRequestRoutes",
  "pendingWorldRequests",
  "pendingWorldTicks",
] as const;

const COORDINATOR_KEYS = [
  "queuedAccounts",
  "connectedAccounts",
  "connectionRoutes",
  "liveMatches",
  "pendingSettlements",
  "pendingDespawns",
  "hardDeadlineTasks",
  "runtimeGenerations",
  "runtimeOwnedMatches",
  "runtimeForcedEliminations",
  "runtimeHardDeadlines",
] as const;

const WORLD_KEYS = [
  "clientCount",
  "entityCount",
  "messageQueueCritical",
  "messageQueueNormal",
  "messageQueueBulk",
  "encodedPending",
  "encodedProcessed",
] as const;
const MEMORY_KEYS = [
  "liveAllocatedBytes",
  "peakAllocatedBytes",
  "liveAllocations",
  "allocationCount",
  "deallocationCount",
  "reallocationCount",
  "liveWorldInstances",
  "worldBackgroundTasks",
] as const;
const MAX_POLL_REQUEST_MS = 5_000;
const RESOURCE_STABILITY_INTERVAL_MS = 125;
const REQUIRED_STABLE_SNAPSHOTS = 3;

type ServerCountKey = (typeof SERVER_KEYS)[number];
type CoordinatorCountKey = (typeof COORDINATOR_KEYS)[number];
type WorldCountKey = (typeof WORLD_KEYS)[number];
type MemoryCountKey = (typeof MEMORY_KEYS)[number];

export type ServerResourceCounts = Record<ServerCountKey, number>;
export type CoordinatorResourceCounts = Record<CoordinatorCountKey, number> & {
  tickerPending: boolean;
};
export type WorldResourceCounts = Record<WorldCountKey, number>;
export type MemoryResourceCounts = Record<MemoryCountKey, number>;

export interface LiveResourceSnapshot {
  coordinator: CoordinatorResourceCounts;
  memory: MemoryResourceCounts;
  server: ServerResourceCounts;
  worlds: WorldResourceCounts[];
}

export function fetchLiveResourceSnapshot(
  transport: LiveHttpTransport,
  timeoutMs?: number,
): Promise<LiveResourceSnapshot> {
  return requestJson(
    transport,
    "/api/e2e/resources",
    { method: "GET", timeoutMs },
    decodeLiveResourceSnapshot,
  );
}

export function decodeLiveResourceSnapshot(
  value: unknown,
): LiveResourceSnapshot {
  const source = readRecord(value, "e2eResources");
  assertOnlyKeys(
    source,
    ["server", "coordinator", "memory", "worlds"],
    "e2eResources",
  );
  return {
    server: decodeCounts(source.server, SERVER_KEYS, "e2eResources.server"),
    coordinator: decodeCoordinator(source.coordinator),
    memory: decodeCounts(source.memory, MEMORY_KEYS, "e2eResources.memory"),
    worlds: readArray(source.worlds, "e2eResources.worlds").map(
      (world, index) =>
        decodeCounts(world, WORLD_KEYS, `e2eResources.worlds[${index}]`),
    ),
  };
}

export function isFullyReleased(snapshot: LiveResourceSnapshot): boolean {
  return (
    snapshot.worlds.length === 0 &&
    snapshot.memory.liveWorldInstances === 0 &&
    snapshot.memory.worldBackgroundTasks === 0 &&
    Object.values(snapshot.server).every((count) => count === 0) &&
    !snapshot.coordinator.tickerPending &&
    COORDINATOR_KEYS.every((key) => snapshot.coordinator[key] === 0)
  );
}

export function isMatchReleasedWithLobbyConnections(
  snapshot: LiveResourceSnapshot,
  expectedConnections: number,
): boolean {
  if (!Number.isSafeInteger(expectedConnections) || expectedConnections < 0) {
    throw new Error(
      "expected lobby connections must be a non-negative integer",
    );
  }
  return (
    snapshot.worlds.length === 0 &&
    snapshot.memory.liveWorldInstances === 0 &&
    snapshot.memory.worldBackgroundTasks === 0 &&
    SERVER_KEYS.every((key) => {
      const expected =
        key === "lostSessions" || key === "connectionPrincipals"
          ? expectedConnections
          : 0;
      return snapshot.server[key] === expected;
    }) &&
    COORDINATOR_KEYS.every((key) => {
      const expected =
        key === "connectedAccounts" || key === "connectionRoutes"
          ? expectedConnections
          : 0;
      return snapshot.coordinator[key] === expected;
    }) &&
    !snapshot.coordinator.tickerPending
  );
}

export function pollUntilMatchReleasedWithLobbyConnections(
  transport: LiveHttpTransport,
  expectedConnections: number,
  timeoutMs: number,
  label: string,
): Promise<LiveResourceSnapshot> {
  return pollUntilResourceState(transport, timeoutMs, label, (snapshot) =>
    isMatchReleasedWithLobbyConnections(snapshot, expectedConnections),
  );
}

export async function pollUntilFullyReleased(
  transport: LiveHttpTransport,
  timeoutMs: number,
  label: string,
): Promise<LiveResourceSnapshot> {
  return pollUntilResourceState(transport, timeoutMs, label, isFullyReleased);
}

async function pollUntilResourceState(
  transport: LiveHttpTransport,
  timeoutMs: number,
  label: string,
  predicate: (snapshot: LiveResourceSnapshot) => boolean,
): Promise<LiveResourceSnapshot> {
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0) {
    throw new Error("resource poll timeout must be a positive safe integer");
  }
  const deadline = Date.now() + timeoutMs;
  let lastSnapshot: LiveResourceSnapshot | null = null;
  let lastTransientError: Error | null = null;
  let consecutiveMatches = 0;
  for (;;) {
    const remainingMs = deadline - Date.now();
    if (remainingMs <= 0) {
      throw resourceDeadlineError(label, lastSnapshot, lastTransientError);
    }
    try {
      lastSnapshot = await fetchLiveResourceSnapshot(
        transport,
        Math.min(remainingMs, MAX_POLL_REQUEST_MS),
      );
      lastTransientError = null;
      consecutiveMatches = predicate(lastSnapshot) ? consecutiveMatches + 1 : 0;
      if (consecutiveMatches >= REQUIRED_STABLE_SNAPSHOTS) {
        return lastSnapshot;
      }
    } catch (error) {
      if (!isTransientResourceReadError(error)) throw error;
      consecutiveMatches = 0;
      lastTransientError =
        error instanceof Error ? error : new Error(String(error));
    }
    const waitMs = Math.min(
      RESOURCE_STABILITY_INTERVAL_MS,
      deadline - Date.now(),
    );
    if (waitMs <= 0) {
      throw resourceDeadlineError(label, lastSnapshot, lastTransientError);
    }
    await new Promise((resolve) => setTimeout(resolve, waitMs));
  }
}

function isTransientResourceReadError(error: unknown): boolean {
  if (error instanceof LiveApiError) return error.retryable;
  if (typeof error !== "object" || error === null) return false;
  const candidate = error as { message?: unknown; name?: unknown };
  if (candidate.name === "TimeoutError") return true;
  return (
    typeof candidate.message === "string" &&
    /^apiRequestContext\.fetch: Timeout \d+ms exceeded\./u.test(
      candidate.message,
    )
  );
}

function resourceDeadlineError(
  label: string,
  lastSnapshot: LiveResourceSnapshot | null,
  lastTransientError: Error | null,
): Error {
  const detail =
    lastSnapshot === null
      ? lastTransientError?.message ?? "no resource snapshot"
      : JSON.stringify(lastSnapshot);
  return new Error(
    `${label}: resources did not reach expected baseline: ${detail}`,
  );
}

function decodeCoordinator(value: unknown): CoordinatorResourceCounts {
  const path = "e2eResources.coordinator";
  const source = readRecord(value, path);
  assertOnlyKeys(source, [...COORDINATOR_KEYS, "tickerPending"], path);
  return {
    ...readCountFields(source, COORDINATOR_KEYS, path),
    tickerPending: readBoolean(source.tickerPending, `${path}.tickerPending`),
  };
}

function decodeCounts<const Key extends string>(
  value: unknown,
  keys: readonly Key[],
  path: string,
): Record<Key, number> {
  const source = readRecord(value, path);
  assertOnlyKeys(source, [...keys], path);
  return readCountFields(source, keys, path);
}

function readCountFields<const Key extends string>(
  source: ReturnType<typeof readRecord>,
  keys: readonly Key[],
  path: string,
): Record<Key, number> {
  return Object.fromEntries(
    keys.map((key) => [
      key,
      readUnsignedInteger(source[key], `${path}.${key}`),
    ]),
  ) as Record<Key, number>;
}
