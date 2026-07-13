import { describe, expect, it, vi } from "vitest";

import type { MiningStateEnvelope } from "../../../contracts/extraction/v1/typescript";
import {
  authoritativeMiningProgress,
  INITIAL_MINING_VIEW_STATE,
  reduceMiningState,
} from "./mining-state";

const MATCH_ID = "33333333-3333-4333-8333-333333333333";

function miningState(revision: number, elapsedMs: number): MiningStateEnvelope {
  return {
    protocolVersion: 1,
    type: "state",
    matchId: MATCH_ID,
    stream: "mining",
    revision,
    data: {
      status: "mining",
      acceptedSequence: revision,
      target: [4, 28, -9],
      resource: "gold",
      elapsedMs,
      requiredMs: 1500,
    },
  };
}

describe("authoritative mining state", () => {
  it("忽略旧 revision，并接受合并后的更高完整快照", () => {
    const first = reduceMiningState(
      INITIAL_MINING_VIEW_STATE,
      miningState(4, 500),
      MATCH_ID,
    );
    expect(reduceMiningState(first, miningState(3, 900), MATCH_ID)).toBe(first);

    const jumped = reduceMiningState(first, miningState(7, 750), MATCH_ID);
    expect(jumped.snapshot?.revision).toBe(7);
  });

  it("ignores another match and never advances from a local timer", () => {
    vi.useFakeTimers();
    const state = reduceMiningState(
      INITIAL_MINING_VIEW_STATE,
      miningState(1, 250),
      MATCH_ID,
    );
    vi.advanceTimersByTime(5000);
    expect(authoritativeMiningProgress(state)).toEqual({
      elapsedMs: 250,
      requiredMs: 1500,
    });

    const other = { ...miningState(2, 500), matchId: crypto.randomUUID() };
    expect(reduceMiningState(state, other, MATCH_ID)).toBe(state);
    vi.useRealTimers();
  });
});
