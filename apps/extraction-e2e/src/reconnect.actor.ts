import { describe, expect, it } from "vitest";

import type { GameplayStateData } from "../../../contracts/extraction/v1/typescript";
import type { MatchResult } from "../../extraction-client/src/api/models";

import {
  assertRecoveredStatePreserved,
  assertUniqueTerminalOutcome,
} from "./live/reconnect-assertions";

const matchId = "00000000-0000-4000-8000-000000000061";
const victimPlayerId = "victim-public-id";
const killerPlayerId = "killer-public-id";

describe("live reconnect acceptance rules", () => {
  it("accepts the same character, health and inventory after rebind", () => {
    const before = gameplayState();
    const after = structuredClone(before);
    expect(() =>
      assertRecoveredStatePreserved({
        after,
        afterPlayerId: victimPlayerId,
        afterPosition: [1.1, 50, 2.1],
        before,
        beforePlayerId: victimPlayerId,
        beforePosition: [1, 50, 2],
      }),
    ).not.toThrow();
  });

  it("rejects a copied slot, reset health or replacement character", () => {
    const before = gameplayState();
    const copied = gameplayState();
    copied.inventory.slots[1] = { resource: "dirt", quantity: 1 };
    expect(() =>
      assertRecoveredStatePreserved({
        after: copied,
        afterPlayerId: victimPlayerId,
        afterPosition: [1, 50, 2],
        before,
        beforePlayerId: victimPlayerId,
        beforePosition: [1, 50, 2],
      }),
    ).toThrow(/inventory/u);

    const healed = gameplayState();
    healed.health.data.currentHalfHearts = 20;
    expect(() =>
      assertRecoveredStatePreserved({
        after: healed,
        afterPlayerId: victimPlayerId,
        afterPosition: [1, 50, 2],
        before,
        beforePlayerId: victimPlayerId,
        beforePosition: [1, 50, 2],
      }),
    ).toThrow(/health/u);

    expect(() =>
      assertRecoveredStatePreserved({
        after: gameplayState(),
        afterPlayerId: "replacement-player",
        afterPosition: [1, 50, 2],
        before,
        beforePlayerId: victimPlayerId,
        beforePosition: [1, 50, 2],
      }),
    ).toThrow(/player ID/u);
  });

  it.each([
    ["dead", "melee", killerPlayerId],
    ["timedOut", "reconnectTimeout", null],
  ] as const)(
    "accepts one %s result and one deterministic death drop",
    (status, cause, killer) => {
      const result = terminalResult(status, cause, killer);
      expect(() =>
        assertUniqueTerminalOutcome({
          expectedCause: cause,
          expectedKillerPlayerId: killer,
          expectedLostDirt: 1,
          expectedStatus: status,
          firstResult: result,
          lootCreationCount: 1,
          matchId,
          observedLootIds: [`drop:v1:${matchId}:seat:4:death`],
          repeatedResult: structuredClone(result),
          victimPlayerId,
          visibleLoot: [lootEntity()],
        }),
      ).not.toThrow();
    },
  );

  it("rejects a second drop or a changed terminal result", () => {
    const result = terminalResult("timedOut", "reconnectTimeout", null);
    expect(() =>
      assertUniqueTerminalOutcome({
        expectedCause: "reconnectTimeout",
        expectedKillerPlayerId: null,
        expectedLostDirt: 1,
        expectedStatus: "timedOut",
        firstResult: result,
        lootCreationCount: 1,
        matchId,
        observedLootIds: [
          `drop:v1:${matchId}:seat:4:death`,
          `drop:v1:${matchId}:seat:4:death-copy`,
        ],
        repeatedResult: result,
        victimPlayerId,
        visibleLoot: [lootEntity()],
      }),
    ).toThrow(/exactly one/u);

    const changed = structuredClone(result);
    changed.stats.lost.dirt = 2;
    expect(() =>
      assertUniqueTerminalOutcome({
        expectedCause: "reconnectTimeout",
        expectedKillerPlayerId: null,
        expectedLostDirt: 1,
        expectedStatus: "timedOut",
        firstResult: result,
        lootCreationCount: 1,
        matchId,
        observedLootIds: [`drop:v1:${matchId}:seat:4:death`],
        repeatedResult: changed,
        victimPlayerId,
        visibleLoot: [lootEntity()],
      }),
    ).toThrow(/changed/u);

    expect(() =>
      assertUniqueTerminalOutcome({
        expectedCause: "reconnectTimeout",
        expectedKillerPlayerId: null,
        expectedLostDirt: 1,
        expectedStatus: "timedOut",
        firstResult: result,
        lootCreationCount: 2,
        matchId,
        observedLootIds: [`drop:v1:${matchId}:seat:4:death`],
        repeatedResult: result,
        victimPlayerId,
        visibleLoot: [lootEntity()],
      }),
    ).toThrow(/duplicate loot CREATE/u);
  });
});

function gameplayState(): GameplayStateData {
  const inventory = {
    slots: [
      { resource: "dirt" as const, quantity: 1 },
      ...Array<null>(11).fill(null),
    ],
    revision: 1,
    frozen: false,
    lastDropSequence: null,
  };
  return {
    matchId,
    inventory,
    equipment: {
      pickaxe: "basic_pickaxe",
      meleeWeapon: "basic_melee_weapon",
    },
    mining: {
      protocolVersion: 1,
      type: "state",
      matchId,
      stream: "mining",
      revision: 1,
      data: { status: "idle", acceptedSequence: 1, reason: "completed" },
    },
    extraction: {
      protocolVersion: 1,
      type: "state",
      matchId,
      stream: "extraction",
      revision: 0,
      data: {
        status: "hidden",
        extractionOpenAtUnixSeconds: 1_800_000_480,
        hardDeadlineUnixSeconds: 1_800_000_720,
      },
    },
    health: {
      protocolVersion: 1,
      type: "state",
      matchId,
      stream: "health",
      revision: 1,
      data: { status: "alive", currentHalfHearts: 18, maxHalfHearts: 20 },
    },
    attack: { revision: 0, acceptedSequence: null },
    deathResult: null,
  };
}

function terminalResult(
  status: "dead" | "timedOut",
  cause: "melee" | "reconnectTimeout",
  killer: string | null,
): MatchResult {
  return {
    matchId,
    status,
    publicPlayerId: victimPlayerId,
    terminalCause: cause,
    killerPublicPlayerId: killer,
    terminalAt: "2030-01-01T00:01:00Z",
    survivedMs: 60_000,
    stats: {
      mined: { dirt: 1, gold: 0, diamond: 0 },
      pickedUp: { dirt: 0, gold: 0, diamond: 0 },
      lost: { dirt: 1, gold: 0, diamond: 0 },
    },
    settlement: null,
  };
}

function lootEntity() {
  const id = `drop:v1:${matchId}:seat:4:death`;
  return {
    id,
    metadata: {
      loot: {
        id,
        contents: { dirt: 1, gold: 0, diamond: 0 },
        revision: 0,
      },
    },
    operation: "CREATE" as const,
    type: "extraction:loot",
  };
}
