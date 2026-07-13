import { describe, expect, it } from "vitest";

import gameplayFixtures from "../../../../contracts/extraction/v1/fixtures/get-state-results.json";
import manifestJson from "../../../../contracts/extraction/v1/manifest.json";
import {
  decodeExtractionManifest,
  decodeGameplayStateData,
} from "../../../../contracts/extraction/v1/typescript";
import type { MatchResult, QueueSnapshot } from "../api/models";

import { INITIAL_APP_STATE, reduceAppState } from "./state";

const manifest = decodeExtractionManifest(manifestJson);
const gameplay = decodeGameplayStateData(
  gameplayFixtures.cases.find(
    ({ name }) => name === "valid-alive-gameplay-state",
  )?.value,
  manifest,
);
const authenticatedState = {
  ...INITIAL_APP_STATE,
  session: {
    address: "0x1111111111111111111111111111111111111111",
    chainId: 1 as const,
  },
};

describe("app state", () => {
  it("does not treat a connected wallet as an authenticated session", () => {
    const booted = reduceAppState(INITIAL_APP_STATE, {
      type: "BOOTSTRAP_READY",
      manifest,
      session: null,
    });
    const connected = reduceAppState(booted, {
      type: "WALLET_CHANGED",
      connected: true,
      address: "0x1111111111111111111111111111111111111111",
      chainId: 1,
    });

    expect(connected.screen).toBe("unauthenticated");
    expect(connected.session).toBeNull();
  });

  it("restores an assigned match from a read-only queue snapshot", () => {
    const queue: QueueSnapshot = {
      status: "active",
      matchId: "22222222-2222-4222-8222-222222222222",
      worldName: "match:v1:22222222",
    };
    const restored = reduceAppState(authenticatedState, {
      type: "QUEUE_CHANGED",
      queue,
    });

    expect(restored.screen).toBe("match");
    expect(restored.activeMatchId).toBe(queue.matchId);
    expect(restored.worldName).toBe(queue.worldName);
    expect(restored.connection).toBe("connecting");
  });

  it("routes settling assignments to result reconciliation without joining", () => {
    const queue: QueueSnapshot = {
      status: "settling",
      matchId: "22222222-2222-4222-8222-222222222222",
      worldName: "match:v1:22222222",
    };
    const restored = reduceAppState(authenticatedState, {
      type: "QUEUE_CHANGED",
      queue,
    });

    expect(restored.screen).toBe("lobby");
    expect(restored.activeMatchId).toBe(queue.matchId);
    expect(restored.worldName).toBeNull();
    expect(restored.connection).toBe("offline");
  });

  it("clears all privileged and control state on logout", () => {
    const active = {
      ...INITIAL_APP_STATE,
      screen: "match" as const,
      session: {
        address: "0x1111111111111111111111111111111111111111",
        chainId: 1 as const,
      },
      activeMatchId: "22222222-2222-4222-8222-222222222222",
      worldName: "match:v1:22222222",
      connection: "online" as const,
    };

    const cleared = reduceAppState(active, {
      type: "SESSION_CLEARED",
      notice: "钱包已变更，请重新登录",
    });

    expect(cleared.screen).toBe("unauthenticated");
    expect(cleared.session).toBeNull();
    expect(cleared.activeMatchId).toBeNull();
    expect(cleared.connection).toBe("offline");
  });

  it("ignores privileged responses that arrive after logout", () => {
    const cleared = reduceAppState(authenticatedState, {
      type: "SESSION_CLEARED",
    });
    const lateQueue = reduceAppState(cleared, {
      type: "QUEUE_CHANGED",
      queue: {
        status: "active",
        matchId: "22222222-2222-4222-8222-222222222222",
        worldName: "match:v1:22222222",
      },
    });
    const lateResult = reduceAppState(lateQueue, {
      type: "MATCH_RESULT",
      result: matchResult("extracted"),
    });

    expect(lateResult).toEqual(cleared);
  });

  it("never regresses a final result to pending reconciliation", () => {
    const extracted = reduceAppState(authenticatedState, {
      type: "MATCH_RESULT",
      result: matchResult("extracted"),
    });
    const stalePending = reduceAppState(extracted, {
      type: "MATCH_RESULT",
      result: matchResult("pendingReconciliation"),
    });

    expect(stalePending).toBe(extracted);
  });

  it("rejects a stale latest-result response after final reconciliation", () => {
    const extracted = reduceAppState(authenticatedState, {
      type: "MATCH_RESULT",
      result: matchResult("extracted"),
    });
    const staleLatest = reduceAppState(extracted, {
      type: "LATEST_RESULT",
      result: matchResult("pendingReconciliation"),
    });
    const staleNull = reduceAppState(extracted, {
      type: "LATEST_RESULT",
      result: null,
    });

    expect(staleLatest).toBe(extracted);
    expect(staleNull).toBe(extracted);
  });

  it("does not route a terminal player back into the old active world", () => {
    const extracted = reduceAppState(authenticatedState, {
      type: "MATCH_RESULT",
      result: matchResult("extracted"),
    });
    const staleAssignment = reduceAppState(extracted, {
      type: "QUEUE_CHANGED",
      queue: {
        status: "active",
        matchId: "22222222-2222-4222-8222-222222222222",
        worldName: "match:v1:old-world",
      },
    });

    expect(staleAssignment).toMatchObject({
      screen: "lobby",
      activeMatchId: null,
      worldName: null,
      gameplay: null,
    });
  });

  it("clears a stale realtime failure notice after an authoritative snapshot", () => {
    const active = {
      ...authenticatedState,
      screen: "match" as const,
      activeMatchId: gameplay.matchId,
      notice: "实时状态刷新失败，正在等待重试",
    };
    const recovered = reduceAppState(active, {
      type: "GAMEPLAY_STATE",
      state: gameplay,
    });
    const duplicateFailure = reduceAppState(
      { ...recovered, notice: "实时状态刷新失败，正在等待重试" },
      { type: "GAMEPLAY_STATE", state: gameplay },
    );

    expect(recovered.notice).toBeNull();
    expect(recovered.gameplay).toEqual(gameplay);
    expect(duplicateFailure.notice).toBeNull();
  });

  it("clears a terminal transport race after result and lobby recovery", () => {
    const failed = {
      ...authenticatedState,
      screen: "match" as const,
      notice: "服务端拒绝了实时请求",
    };
    const terminal = reduceAppState(failed, {
      type: "MATCH_RESULT",
      result: matchResult("timedOut"),
    });
    const lateFailure = { ...terminal, notice: "服务端拒绝了实时请求" };
    const lobby = reduceAppState(lateFailure, {
      type: "QUEUE_CHANGED",
      queue: { status: "idle" },
    });

    expect(terminal.notice).toBeNull();
    expect(lobby.screen).toBe("lobby");
    expect(lobby.notice).toBeNull();
  });
});

function matchResult(status: MatchResult["status"]): MatchResult {
  const zero = { dirt: 0, gold: 0, diamond: 0 };
  return {
    matchId: "22222222-2222-4222-8222-222222222222",
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
