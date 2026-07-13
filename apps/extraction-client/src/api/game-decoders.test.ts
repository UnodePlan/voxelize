import { describe, expect, it } from "vitest";

import {
  decodeOptionalMatchResult,
  decodeQueueSnapshot,
  decodeWarehouse,
} from "./game-decoders";

const MATCH_ID = "11111111-1111-4111-8111-111111111111";
const PLAYER_ID = "22222222-2222-4222-8222-222222222222";
const SETTLEMENT_ID = "33333333-3333-4333-8333-333333333333";
const EMPTY_COUNTS = { dirt: 0, gold: 0, diamond: 0 };

describe("game API decoders", () => {
  it("accepts a resumable assigned queue snapshot", () => {
    expect(
      decodeQueueSnapshot({
        status: "active",
        matchId: MATCH_ID,
        worldName: `match:v1:${MATCH_ID}`,
      }),
    ).toMatchObject({ status: "active", matchId: MATCH_ID });
  });

  it("rejects an incomplete world assignment", () => {
    expect(() =>
      decodeQueueSnapshot({ status: "active", matchId: MATCH_ID }),
    ).toThrow("must appear together");
  });

  it("rejects unsafe permanent balances", () => {
    expect(() =>
      decodeWarehouse({
        resources: { ...EMPTY_COUNTS, diamond: Number.MAX_SAFE_INTEGER + 1 },
        stats: {
          totalResourcesExtracted: 0,
          totalExtractionValue: 0,
          successfulExtractions: 0,
          highestSingleMatchValue: 0,
        },
      }),
    ).toThrow("safe integer");
  });

  it("accepts committed extraction and rejects false extraction", () => {
    const base = {
      matchId: MATCH_ID,
      publicPlayerId: PLAYER_ID,
      terminalCause: null,
      killerPublicPlayerId: null,
      terminalAt: null,
      survivedMs: null,
      stats: {
        mined: EMPTY_COUNTS,
        pickedUp: EMPTY_COUNTS,
        lost: EMPTY_COUNTS,
      },
    };
    const settlement = {
      settlementId: SETTLEMENT_ID,
      resources: { dirt: 1, gold: 2, diamond: 3 },
      totalValue: 321,
      configVersion: "extraction-pvp-v1",
      committedAt: "2026-07-13T08:00:00Z",
    };
    expect(
      decodeOptionalMatchResult({ ...base, status: "extracted", settlement }),
    ).toMatchObject({ status: "extracted", settlement });
    expect(() =>
      decodeOptionalMatchResult({
        ...base,
        status: "extracted",
        settlement: null,
      }),
    ).toThrow("inconsistent terminal shape");
  });

  it("keeps pending reconciliation distinct from permanent success", () => {
    expect(
      decodeOptionalMatchResult({
        matchId: MATCH_ID,
        status: "pendingReconciliation",
        publicPlayerId: PLAYER_ID,
        terminalCause: null,
        killerPublicPlayerId: null,
        terminalAt: null,
        survivedMs: null,
        stats: {
          mined: EMPTY_COUNTS,
          pickedUp: EMPTY_COUNTS,
          lost: EMPTY_COUNTS,
        },
        settlement: null,
      }),
    ).toMatchObject({ status: "pendingReconciliation", settlement: null });
  });
});
