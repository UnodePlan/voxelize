import { describe, expect, it } from "vitest";

import type { LiveHttpTransport } from "./live/http";
import { releaseLifecycleProfile } from "./live/release-lifecycle-profile";
import {
  decodeLiveResourceSnapshot,
  isFullyReleased,
  isMatchReleasedWithLobbyConnections,
  pollUntilFullyReleased,
} from "./live/resource-snapshot";
import {
  assessMemoryTrend,
  FINAL_GROWTH_LIMIT_BYTES,
  LINEAR_GROWTH_LIMIT_BYTES,
} from "./live/rss-gate";

const MEBIBYTE = 1_024 * 1_024;

describe("live resource and memory gates", () => {
  it("strictly decodes a fully released count-only snapshot", () => {
    const snapshot = decodeLiveResourceSnapshot(releasedPayload());
    expect(isFullyReleased(snapshot)).toBe(true);
    expect(snapshot.worlds).toEqual([]);
  });

  it("distinguishes a released match with ten reusable lobby sockets", () => {
    const payload = releasedPayload();
    payload.server.lostSessions = 10;
    payload.server.connectionPrincipals = 10;
    payload.coordinator.connectedAccounts = 10;
    payload.coordinator.connectionRoutes = 10;
    const snapshot = decodeLiveResourceSnapshot(payload);

    expect(isMatchReleasedWithLobbyConnections(snapshot, 10)).toBe(true);
    expect(isFullyReleased(snapshot)).toBe(false);
  });

  it("does not report release before the World and background work are dropped", () => {
    for (const key of ["liveWorldInstances", "worldBackgroundTasks"] as const) {
      const payload = releasedPayload();
      payload.memory[key] = 1;
      const snapshot = decodeLiveResourceSnapshot(payload);
      expect(isFullyReleased(snapshot)).toBe(false);
      expect(isMatchReleasedWithLobbyConnections(snapshot, 0)).toBe(false);
    }
  });

  it("keeps allocator diagnostics out of the cross-round release profile", () => {
    const baseline = decodeLiveResourceSnapshot(releasedPayload());
    const laterPayload = releasedPayload();
    laterPayload.memory.liveAllocatedBytes += 128_000;
    laterPayload.memory.peakAllocatedBytes += 8_000_000;
    laterPayload.memory.liveAllocations += 500;
    laterPayload.memory.allocationCount += 300_000;
    laterPayload.memory.deallocationCount += 299_500;
    laterPayload.memory.reallocationCount += 10_000;
    const later = decodeLiveResourceSnapshot(laterPayload);

    expect(releaseLifecycleProfile(later)).toEqual(
      releaseLifecycleProfile(baseline),
    );

    laterPayload.memory.worldBackgroundTasks = 1;
    expect(
      releaseLifecycleProfile(decodeLiveResourceSnapshot(laterPayload)),
    ).not.toEqual(releaseLifecycleProfile(baseline));
  });

  it("bounds each resource request by the poll's remaining deadline", async () => {
    const observedTimeouts: Array<number | undefined> = [];
    const transport: LiveHttpTransport = {
      async request(_path, request) {
        observedTimeouts.push(request.timeoutMs);
        return { body: releasedPayload(), setCookie: null, status: 200 };
      },
    };

    await pollUntilFullyReleased(transport, 1_000, "test baseline");
    expect(observedTimeouts).toHaveLength(3);
    expect(observedTimeouts.every((timeout) => (timeout ?? 0) > 0)).toBe(true);
    expect(observedTimeouts.every((timeout) => (timeout ?? 0) <= 1_000)).toBe(
      true,
    );
  });

  it("retries explicit fetch and Playwright request timeouts", async () => {
    const timeouts = [
      new DOMException("request timed out", "TimeoutError"),
      new Error("apiRequestContext.fetch: Timeout 5ms exceeded."),
    ];
    for (const timeout of timeouts) {
      let requests = 0;
      const transport: LiveHttpTransport = {
        async request() {
          requests += 1;
          if (requests === 1) throw timeout;
          return { body: releasedPayload(), setCookie: null, status: 200 };
        },
      };

      await pollUntilFullyReleased(transport, 1_000, "timeout recovery");
      expect(requests).toBe(4);
    }
  });

  it("resets stability after a transient zero snapshot rebounds", async () => {
    const snapshots = [
      releasedPayload(),
      activeWorldPayload(),
      releasedPayload(),
      releasedPayload(),
      releasedPayload(),
    ];
    let requests = 0;
    const transport: LiveHttpTransport = {
      async request() {
        const body = snapshots[requests] ?? releasedPayload();
        requests += 1;
        return { body, setCookie: null, status: 200 };
      },
    };

    await pollUntilFullyReleased(transport, 1_000, "stable baseline");
    expect(requests).toBe(5);
  });

  it("rejects unknown resource fields and invalid counts", () => {
    expect(() =>
      decodeLiveResourceSnapshot({ ...releasedPayload(), accountId: "secret" }),
    ).toThrow(/unknown field accountId/u);
    const invalid = releasedPayload();
    invalid.server.connections = -1;
    expect(() => decodeLiveResourceSnapshot(invalid)).toThrow(
      /server.connections/u,
    );
  });

  it("accepts stable, recovering, and bounded live allocations", () => {
    expect(assessMemoryTrend([100, 100, 99]).failure).toBeNull();
    expect(assessMemoryTrend([100, 140, 110]).failure).toBeNull();
    expect(
      assessMemoryTrend([100, 100 + 8 * MEBIBYTE, 100 + 16 * MEBIBYTE]).failure,
    ).toBeNull();
  });

  it("fails sustained growth over 16 MiB and any final growth over 64 MiB", () => {
    expect(
      assessMemoryTrend([
        100,
        100 + 9 * MEBIBYTE,
        100 + LINEAR_GROWTH_LIMIT_BYTES + 1,
      ]).failure,
    ).toBe("linear-growth");
    expect(
      assessMemoryTrend([100, 100 + 40 * MEBIBYTE, 100 + 35 * MEBIBYTE])
        .failure,
    ).toBe("linear-growth");
    expect(
      assessMemoryTrend([
        100,
        100 + 80 * MEBIBYTE,
        100 + FINAL_GROWTH_LIMIT_BYTES + 1,
      ]).failure,
    ).toBe("hard-cap");
  });
});

function releasedPayload() {
  return {
    memory: {
      liveAllocatedBytes: 4_000_000,
      peakAllocatedBytes: 400_000_000,
      liveAllocations: 10_000,
      allocationCount: 1_000_000,
      deallocationCount: 990_000,
      reallocationCount: 50_000,
      liveWorldInstances: 0,
      worldBackgroundTasks: 0,
    },
    server: {
      worlds: 0,
      worldGenerations: 0,
      removingWorlds: 0,
      lostSessions: 0,
      transportSessions: 0,
      connections: 0,
      connectionPrincipals: 0,
      pendingJoins: 0,
      leavingSessions: 0,
      connectionClientIds: 0,
      connectionAttachAttemptIds: 0,
      detachedConnections: 0,
      pendingDetaches: 0,
      pendingRebinds: 0,
      pendingWorldRequestRoutes: 0,
      pendingWorldRequests: 0,
      pendingWorldTicks: 0,
    },
    coordinator: {
      queuedAccounts: 0,
      connectedAccounts: 0,
      connectionRoutes: 0,
      liveMatches: 0,
      pendingSettlements: 0,
      pendingDespawns: 0,
      hardDeadlineTasks: 0,
      runtimeGenerations: 0,
      runtimeOwnedMatches: 0,
      runtimeForcedEliminations: 0,
      runtimeHardDeadlines: 0,
      tickerPending: false,
    },
    worlds: [],
  };
}

function activeWorldPayload() {
  const payload = releasedPayload();
  return {
    ...payload,
    server: { ...payload.server, worlds: 1, worldGenerations: 1 },
    worlds: [
      {
        clientCount: 0,
        entityCount: 0,
        messageQueueCritical: 0,
        messageQueueNormal: 0,
        messageQueueBulk: 0,
        encodedPending: 0,
        encodedProcessed: 0,
      },
    ],
  };
}
