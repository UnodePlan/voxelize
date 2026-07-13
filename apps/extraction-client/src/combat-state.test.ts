import { describe, expect, it } from "vitest";

import type {
  DeathResultEnvelope,
  HealthStateEnvelope,
} from "../../../contracts/extraction/v1/typescript";

import {
  INITIAL_COMBAT_VIEW_STATE,
  projectHearts,
  reduceCombatState,
} from "./combat-state";

const MATCH_ID = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const OTHER_MATCH_ID = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";

describe("combat state reducer", () => {
  it("忽略跨比赛和旧 revision 的生命快照", () => {
    const first = reduceCombatState(
      INITIAL_COMBAT_VIEW_STATE,
      { kind: "health", snapshot: health(3, 18) },
      MATCH_ID,
    );
    expect(
      reduceCombatState(
        first,
        { kind: "health", snapshot: health(4, 16, OTHER_MATCH_ID) },
        MATCH_ID,
      ),
    ).toBe(first);
    expect(
      reduceCombatState(
        first,
        { kind: "health", snapshot: health(2, 20) },
        MATCH_ID,
      ),
    ).toBe(first);
  });

  it("将半心数投影为十颗 Minecraft 风格心", () => {
    const state = reduceCombatState(
      INITIAL_COMBAT_VIEW_STATE,
      { kind: "health", snapshot: health(1, 19) },
      MATCH_ID,
    );

    expect(projectHearts(state)).toEqual([
      "full",
      "full",
      "full",
      "full",
      "full",
      "full",
      "full",
      "full",
      "full",
      "half",
    ]);
  });

  it("锁存死亡终态并拒绝后续存活快照复活", () => {
    const withDeath = reduceCombatState(
      INITIAL_COMBAT_VIEW_STATE,
      { kind: "deathResult", snapshot: deathResult(10) },
      MATCH_ID,
    );
    const dead = reduceCombatState(
      withDeath,
      { kind: "health", snapshot: health(10, 0) },
      MATCH_ID,
    );

    expect(dead.health?.data.status).toBe("dead");
    expect(
      reduceCombatState(
        dead,
        { kind: "health", snapshot: health(11, 20) },
        MATCH_ID,
      ),
    ).toBe(dead);
  });

  it("死亡生命与结果必须共享终态 revision", () => {
    const dead = reduceCombatState(
      INITIAL_COMBAT_VIEW_STATE,
      { kind: "health", snapshot: health(10, 0) },
      MATCH_ID,
    );

    expect(
      reduceCombatState(
        dead,
        { kind: "deathResult", snapshot: deathResult(9) },
        MATCH_ID,
      ),
    ).toBe(dead);
  });

  it("忽略落后于当前存活生命的死亡结果", () => {
    const alive = reduceCombatState(
      INITIAL_COMBAT_VIEW_STATE,
      { kind: "health", snapshot: health(10, 16) },
      MATCH_ID,
    );

    expect(
      reduceCombatState(
        alive,
        { kind: "deathResult", snapshot: deathResult(9) },
        MATCH_ID,
      ),
    ).toBe(alive);
  });
});

function health(
  revision: number,
  currentHalfHearts: number,
  matchId = MATCH_ID,
): HealthStateEnvelope {
  return {
    protocolVersion: 1,
    type: "state",
    matchId,
    stream: "health",
    revision,
    data:
      currentHalfHearts === 0
        ? { status: "dead", currentHalfHearts: 0, maxHalfHearts: 20 }
        : { status: "alive", currentHalfHearts, maxHalfHearts: 20 },
  };
}

function deathResult(revision: number): DeathResultEnvelope {
  const empty = { dirt: 0, gold: 0, diamond: 0 };
  return {
    protocolVersion: 1,
    type: "state",
    matchId: MATCH_ID,
    stream: "deathResult",
    revision,
    data: {
      cause: "reconnectTimeout",
      killerPublicPlayerId: null,
      survivedMs: 100,
      mined: empty,
      pickedUp: empty,
      lost: empty,
    },
  };
}
