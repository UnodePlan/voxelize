import type { QueueSnapshot } from "../../../extraction-client/src/api/models";
import type {
  CapacityAdmission,
  CapacityAdmissionRejected,
} from "../capacity-scenario";

import type { LiveE2eConfig } from "./config";
import { fetchQueue, LiveApiError, type LiveHttpTransport } from "./http";

const ASSIGNED_STATUSES = new Set([
  "preparing",
  "active",
  "extractionOpen",
  "settling",
]);

export class PrimaryAdmissionBarrier {
  private readonly arrived = new Set<string>();
  private release: (() => void) | null = null;
  private readonly ready = new Promise<void>((resolve) => {
    this.release = resolve;
  });

  constructor(private readonly expected: number) {
    if (!Number.isSafeInteger(expected) || expected <= 0) {
      throw new Error("admission barrier expected count must be positive");
    }
  }

  arrive(actorId: string): void {
    this.arrived.add(actorId);
    if (this.arrived.size === this.expected) this.release?.();
  }

  wait(timeoutMs: number): Promise<void> {
    return withTimeout(
      this.ready,
      timeoutMs,
      `timed out waiting for ${this.expected} primary queue requests`,
    );
  }
}

export async function enqueueProtocolActor(
  actorId: string,
  primary: boolean,
  transport: LiveHttpTransport,
  barrier: PrimaryAdmissionBarrier,
  config: LiveE2eConfig,
): Promise<CapacityAdmission> {
  if (!primary) await barrier.wait(config.scenarioTimeoutMs);
  let initial: QueueSnapshot | CapacityAdmissionRejected;
  try {
    initial = await postQueueAfterSocketRegistration(transport, config);
  } finally {
    if (primary) barrier.arrive(actorId);
  }
  if (initial.status === "rejected") return initial;
  return waitForAssignment(transport, initial, config);
}

export async function observeBrowserAdmission(
  actorId: string,
  transport: LiveHttpTransport,
  barrier: PrimaryAdmissionBarrier,
  config: LiveE2eConfig,
): Promise<CapacityAdmission> {
  let initial: QueueSnapshot;
  try {
    initial = await pollQueue(
      transport,
      (snapshot) => snapshot.status !== "idle",
      config,
      "browser queue action did not reach the server",
    );
  } finally {
    barrier.arrive(actorId);
  }
  return waitForAssignment(transport, initial, config);
}

export async function readQueueAssignment(
  transport: LiveHttpTransport,
  config: LiveE2eConfig,
): Promise<CapacityAdmission> {
  return waitForAssignment(
    transport,
    await fetchQueue(transport, "GET"),
    config,
  );
}

async function postQueueAfterSocketRegistration(
  transport: LiveHttpTransport,
  config: LiveE2eConfig,
): Promise<QueueSnapshot | CapacityAdmissionRejected> {
  const deadline = Date.now() + config.connectionGraceMs;
  for (;;) {
    try {
      return await fetchQueue(transport, "POST");
    } catch (error) {
      if (error instanceof LiveApiError && error.code === "MATCH_FULL") {
        return { status: "rejected", code: "MATCH_FULL" };
      }
      // WebSocket upgrade 可早于服务端连接观察事件完成，短暂重试该唯一竞态。
      if (
        error instanceof LiveApiError &&
        error.code === "MATCH_ROSTER_LOCKED" &&
        Date.now() < deadline
      ) {
        await sleep(config.pollIntervalMs);
        continue;
      }
      throw error;
    }
  }
}

async function waitForAssignment(
  transport: LiveHttpTransport,
  initial: QueueSnapshot,
  config: LiveE2eConfig,
): Promise<CapacityAdmission> {
  const assigned = assignment(initial);
  if (assigned !== null) return assigned;
  if (initial.status !== "queued") {
    throw new Error(
      `unexpected queue status before assignment: ${initial.status}`,
    );
  }
  const snapshot = await pollQueue(
    transport,
    (value) => assignment(value) !== null,
    config,
    "timed out waiting for a frozen match assignment",
  );
  const result = assignment(snapshot);
  if (result === null) throw new Error("queue assignment disappeared");
  return result;
}

function assignment(snapshot: QueueSnapshot): CapacityAdmission | null {
  if (!ASSIGNED_STATUSES.has(snapshot.status)) return null;
  if (snapshot.matchId === undefined || snapshot.worldName === undefined) {
    throw new Error("assigned queue snapshot omitted matchId or worldName");
  }
  return {
    status: "accepted",
    matchId: snapshot.matchId,
    worldName: snapshot.worldName,
  };
}

async function pollQueue(
  transport: LiveHttpTransport,
  predicate: (snapshot: QueueSnapshot) => boolean,
  config: LiveE2eConfig,
  timeoutMessage: string,
): Promise<QueueSnapshot> {
  const deadline = Date.now() + config.scenarioTimeoutMs;
  for (;;) {
    const snapshot = await fetchQueue(transport, "GET");
    if (predicate(snapshot)) return snapshot;
    if (Date.now() >= deadline) throw new Error(timeoutMessage);
    await sleep(config.pollIntervalMs);
  }
}

function sleep(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  message: string,
): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | null = null;
  try {
    return await Promise.race([
      promise,
      new Promise<never>((_, reject) => {
        timer = setTimeout(() => reject(new Error(message)), timeoutMs);
      }),
    ]);
  } finally {
    if (timer !== null) clearTimeout(timer);
  }
}
