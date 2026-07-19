import { describe, expect, it } from "vitest";

import type { GameplayStateData } from "../../../../contracts/extraction/v1/typescript";

import {
  cuesFromAttackResolution,
  cuesFromGameplayTransition,
} from "./match-sfx-feedback";

function baseState(
  overrides: Partial<GameplayStateData> = {},
): GameplayStateData {
  const healthAlive = {
    protocolVersion: 1,
    type: "state" as const,
    matchId: "m1",
    stream: "health" as const,
    revision: 1,
    data: {
      status: "alive" as const,
      currentHalfHearts: 20,
      maxHalfHearts: 20 as const,
    },
  };
  return {
    matchId: "m1",
    inventory: {
      slots: Array.from({ length: 12 }, () => null),
      revision: 0,
      frozen: false,
      lastDropSequence: null,
    },
    equipment: {
      pickaxe: "basic_pickaxe",
      meleeWeapon: "basic_melee_weapon",
    },
    mining: {
      protocolVersion: 1,
      type: "state",
      matchId: "m1",
      stream: "mining",
      revision: 0,
      data: { status: "idle", acceptedSequence: null, reason: "initial" },
    },
    extraction: {
      protocolVersion: 1,
      type: "state",
      matchId: "m1",
      stream: "extraction",
      revision: 0,
      data: {
        status: "hidden",
        extractionOpenAtUnixSeconds: 0,
        hardDeadlineUnixSeconds: 0,
      },
    },
    health: healthAlive,
    attack: { revision: 0, acceptedSequence: null },
    deathResult: null,
    ...overrides,
  };
}

describe("cuesFromGameplayTransition", () => {
  it("emits death and drop when deathResult appears with lost loot", () => {
    const prev = baseState();
    const next = baseState({
      health: {
        ...prev.health,
        revision: 2,
        data: { status: "dead", currentHalfHearts: 0, maxHalfHearts: 20 },
      },
      deathResult: {
        protocolVersion: 1,
        type: "state",
        matchId: "m1",
        stream: "deathResult",
        revision: 2,
        data: {
          cause: "melee",
          killerPublicPlayerId: null,
          survivedMs: 1000,
          mined: { dirt: 0, gold: 0, diamond: 0 },
          pickedUp: { dirt: 0, gold: 0, diamond: 0 },
          lost: { dirt: 2, gold: 1, diamond: 0 },
        },
      },
    });
    expect(cuesFromGameplayTransition(prev, next)).toEqual(["death", "drop"]);
  });

  it("emits hitTaken when half hearts drop", () => {
    const prev = baseState();
    const next = baseState({
      health: {
        ...prev.health,
        revision: 2,
        data: {
          status: "alive",
          currentHalfHearts: 18,
          maxHalfHearts: 20,
        },
      },
    });
    expect(cuesFromGameplayTransition(prev, next)).toContain("hitTaken");
  });

  it("emits drop on lastDropSequence advance", () => {
    const prev = baseState();
    const next = baseState({
      inventory: {
        ...prev.inventory,
        revision: 1,
        lastDropSequence: 3,
        slots: [
          null,
          null,
          null,
          null,
          null,
          null,
          null,
          null,
          null,
          null,
          null,
          null,
        ],
      },
    });
    expect(cuesFromGameplayTransition(prev, next)).toContain("drop");
  });
});

describe("cuesFromAttackResolution", () => {
  it("maps hit and kill", () => {
    expect(cuesFromAttackResolution("miss")).toEqual([]);
    expect(cuesFromAttackResolution("hit")).toEqual(["hit"]);
    expect(cuesFromAttackResolution("kill")).toEqual(["kill", "drop"]);
  });
});
