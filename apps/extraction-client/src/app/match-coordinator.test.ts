import { afterEach, describe, expect, it, vi } from "vitest";

import stateFixtures from "../../../../contracts/extraction/v1/fixtures/get-state-results.json";
import manifestJson from "../../../../contracts/extraction/v1/manifest.json";
import {
  decodeExtractionManifest,
  decodeGameplayStateData,
  type ExtractionManifest,
} from "../../../../contracts/extraction/v1/typescript";
import type { GameApi } from "../api/game";
import type {
  MatchResult,
  QueueSnapshot,
  WarehouseSnapshot,
} from "../api/models";
import type { GameNetworkEvents } from "../game/network";

import { MatchCoordinator } from "./match-coordinator";
import { INITIAL_APP_STATE, reduceAppState, type AppState } from "./state";

const manifest = decodeExtractionManifest(manifestJson);
const gameplay = decodeGameplayStateData(
  stateFixtures.cases.find(({ name }) => name === "valid-alive-gameplay-state")
    ?.value,
  manifest,
);
const warehouse: WarehouseSnapshot = {
  resources: { dirt: 1, gold: 2, diamond: 3 },
  stats: {
    totalResourcesExtracted: 6,
    totalExtractionValue: 321,
    successfulExtractions: 1,
    highestSingleMatchValue: 321,
  },
};

describe("MatchCoordinator", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it("drops lobby responses from a closed authentication generation", async () => {
    const queue = deferred<QueueSnapshot>();
    const warehouseResult = deferred<WarehouseSnapshot>();
    const latest = deferred<MatchResult | null>();
    const harness = coordinatorHarness(
      gameApi({
        getQueue: () => queue.promise,
        getWarehouse: () => warehouseResult.promise,
        getLatestResult: () => latest.promise,
      }),
    );
    harness.coordinator.activate();
    const loading = harness.coordinator.loadLobby();
    harness.coordinator.close();

    queue.resolve({ status: "queued", position: 1 });
    warehouseResult.resolve(warehouse);
    latest.resolve(null);
    await loading;

    expect(harness.state()).toEqual(authenticatedState());
  });

  it("restores an active world even when warehouse loading fails", async () => {
    const network = fakeNetwork();
    const harness = coordinatorHarness(
      gameApi({
        getQueue: async () => activeQueue(),
        getWarehouse: async () =>
          Promise.reject(new Error("warehouse offline")),
        getLatestResult: async () => null,
      }),
      () => network,
    );
    harness.coordinator.activate();

    await harness.coordinator.loadLobby();
    await Promise.resolve();

    expect(network.resume).toHaveBeenCalledWith(activeQueue().worldName);
    expect(network.join).not.toHaveBeenCalled();
    expect(harness.state().screen).toBe("match");
    expect(harness.state().warehouse).toBeNull();
  });

  it("retries authenticated rebind when the first full-state request fails", async () => {
    const network = fakeNetwork();
    network.requestGameplayState.mockRejectedValueOnce(
      new Error("rebind not ready"),
    );
    const harness = coordinatorHarness(
      gameApi({ getQueue: async () => activeQueue() }),
      () => network,
    );
    harness.coordinator.activate();

    await harness.coordinator.loadLobby();
    await vi.waitFor(() => {
      expect(network.retryResume).toHaveBeenCalledTimes(1);
    });
    harness.coordinator.close();
  });

  it("leaves a preparing world when matchmaking returns to queued", async () => {
    const network = fakeNetwork();
    const getQueue = vi
      .fn<[], Promise<QueueSnapshot>>()
      .mockResolvedValueOnce({
        status: "preparing",
        matchId: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        worldName: "match:v1:first",
      })
      .mockResolvedValueOnce({ status: "queued", position: 1 });
    const harness = coordinatorHarness(
      gameApi({ getQueue, getWarehouse: async () => warehouse }),
      () => network,
    );
    harness.coordinator.activate();
    await harness.coordinator.loadLobby();
    await Promise.resolve();
    expect(network.join).toHaveBeenCalledWith("match:v1:first");

    await harness.coordinator.loadLobby();
    expect(network.leave).toHaveBeenCalledTimes(1);
    expect(harness.state()).toMatchObject({
      screen: "queue",
      activeMatchId: null,
      worldName: null,
      gameplay: null,
    });
    harness.coordinator.close();
  });

  it("ignores an in-flight queue poll after leaveQueue begins", async () => {
    vi.useFakeTimers();
    const stalePoll = deferred<QueueSnapshot>();
    const getQueue = vi
      .fn<[], Promise<QueueSnapshot>>()
      .mockResolvedValueOnce({ status: "queued", position: 1 })
      .mockImplementationOnce(() => stalePoll.promise);
    const harness = coordinatorHarness(
      gameApi({
        getQueue,
        leaveQueue: async () => ({ status: "idle", removed: true }),
      }),
    );
    harness.coordinator.activate();
    await harness.coordinator.loadLobby();
    vi.advanceTimersByTime(1_000);
    await Promise.resolve();

    await harness.coordinator.leaveQueue();
    stalePoll.resolve({ ...activeQueue(), status: "preparing" });
    await Promise.resolve();

    expect(harness.state()).toMatchObject({
      screen: "lobby",
      queue: { status: "idle", removed: true },
      activeMatchId: null,
      worldName: null,
    });
    harness.coordinator.close();
  });

  it("ignores match A result after the player starts match B", async () => {
    const oldResult = deferred<MatchResult | null>();
    const matchB: QueueSnapshot = {
      status: "active",
      matchId: "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
      worldName: "match:v1:second",
    };
    const network = fakeNetwork();
    const initial = {
      ...authenticatedState(),
      screen: "result" as const,
      activeMatchId: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
      result: matchResult("pendingReconciliation"),
    };
    const harness = coordinatorHarness(
      gameApi({
        getMatchResult: () => oldResult.promise,
        joinQueue: async () => matchB,
      }),
      () => network,
      initial,
    );
    harness.coordinator.activate();
    const refreshingOldResult = harness.coordinator.refreshResult();
    await harness.coordinator.joinQueue();
    oldResult.resolve(matchResult("extracted"));
    await refreshingOldResult;

    expect(network.leave).not.toHaveBeenCalled();
    expect(harness.state()).toMatchObject({
      screen: "match",
      activeMatchId: matchB.matchId,
      worldName: matchB.worldName,
    });
    harness.coordinator.close();
  });

  it("waits for gameplay state and a rebuilt voxel world before going online", async () => {
    const network = fakeNetwork();
    let events!: GameNetworkEvents;
    const harness = coordinatorHarness(
      gameApi({ getQueue: async () => activeQueue() }),
      (_manifest, incomingEvents) => {
        events = incomingEvents;
        return network;
      },
    );
    harness.coordinator.activate();

    await harness.coordinator.loadLobby();
    await vi.waitFor(() => expect(harness.state().gameplay).not.toBeNull());
    expect(harness.state().connection).toBe("connecting");
    events.onConnection("online");
    expect(harness.state().connection).toBe("connecting");
    harness.coordinator.markWorldReady();
    expect(harness.state().connection).toBe("online");

    events.onConnection("reconnecting");
    events.onVoxelReset?.();
    events.onGameplayState(gameplay);
    events.onConnection("online");
    expect(harness.state().connection).toBe("reconnecting");
    harness.coordinator.markWorldReady();
    expect(harness.state().connection).toBe("online");
    harness.coordinator.close();
  });

  it("disposes the live voxel world after closing the authentication generation", () => {
    const resetWorld = vi.fn();
    let state = authenticatedState();
    const coordinator = new MatchCoordinator({
      game: gameApi({}),
      getState: () => state,
      dispatch: (action) => {
        state = reduceAppState(state, action);
      },
      networkFactory: () => fakeNetwork(),
      onAuthenticationInvalidated: vi.fn(),
      onVoxelReset: resetWorld,
    });
    coordinator.activate();
    resetWorld.mockClear();

    coordinator.close();

    expect(resetWorld).toHaveBeenCalledTimes(1);
  });
});

function coordinatorHarness(
  game: GameApi,
  networkFactory: (
    manifest: ExtractionManifest,
    events: GameNetworkEvents,
  ) => ReturnType<typeof fakeNetwork> = () => fakeNetwork(),
  initialState = authenticatedState(),
) {
  let state = initialState;
  const coordinator = new MatchCoordinator({
    game,
    getState: () => state,
    dispatch: (action) => {
      state = reduceAppState(state, action);
    },
    networkFactory,
    onAuthenticationInvalidated: vi.fn(),
  });
  return { coordinator, state: () => state };
}

function matchResult(status: MatchResult["status"]): MatchResult {
  const zero = { dirt: 0, gold: 0, diamond: 0 };
  return {
    matchId: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    status,
    publicPlayerId: "33333333-3333-4333-8333-333333333333",
    terminalCause: null,
    killerPublicPlayerId: null,
    terminalAt: null,
    survivedMs: null,
    stats: { mined: zero, pickedUp: zero, lost: zero },
    settlement:
      status === "extracted"
        ? {
            settlementId: "44444444-4444-4444-8444-444444444444",
            resources: zero,
            totalValue: 0,
            configVersion: "extraction-pvp-v1",
            committedAt: "2026-07-13T08:12:08Z",
          }
        : null,
  };
}

function authenticatedState(): AppState {
  return {
    ...INITIAL_APP_STATE,
    screen: "lobby",
    manifest,
    session: {
      address: "0x1111111111111111111111111111111111111111",
      chainId: 1,
    },
  };
}

function activeQueue(): QueueSnapshot {
  return {
    status: "active",
    matchId: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    worldName: "match:v1:active",
  };
}

function gameApi(overrides: Partial<GameApi>): GameApi {
  return {
    getQueue: async () => ({ status: "idle" }),
    joinQueue: async () => ({ status: "queued", position: 1 }),
    leaveQueue: async () => ({ status: "idle" }),
    getWarehouse: async () => warehouse,
    getLatestResult: async () => null,
    getMatchResult: async () => null,
    ...overrides,
  };
}

function fakeNetwork() {
  return {
    close: vi.fn(),
    connect: vi.fn(async () => undefined),
    join: vi.fn(),
    leave: vi.fn(),
    requestGameplayState: vi.fn(async () => gameplay),
    resume: vi.fn(),
    retryResume: vi.fn(),
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}
