import { isDeepStrictEqual } from "node:util";

import type { GameplayStateData } from "../../../../contracts/extraction/v1/typescript";
import type { MatchResult } from "../../../extraction-client/src/api/models";

import { readRecord, type LiveVector3 } from "./gameplay-state";
import type { LiveEntity } from "./wire";

export interface RecoveredStateEvidence {
  after: GameplayStateData;
  afterPlayerId: string;
  afterPosition: LiveVector3;
  before: GameplayStateData;
  beforePlayerId: string;
  beforePosition: LiveVector3;
}

export interface UniqueTerminalEvidence {
  expectedCause: "melee" | "reconnectTimeout";
  expectedKillerPlayerId: string | null;
  expectedLostDirt: number;
  expectedStatus: "dead" | "timedOut";
  firstResult: MatchResult;
  matchId: string;
  lootCreationCount: number;
  observedLootIds: readonly string[];
  repeatedResult: MatchResult;
  victimPlayerId: string;
  visibleLoot: readonly LiveEntity[];
}

export function assertRecoveredStatePreserved(
  evidence: RecoveredStateEvidence,
): void {
  const { after, before } = evidence;
  if (
    evidence.beforePlayerId === "" ||
    evidence.afterPlayerId !== evidence.beforePlayerId
  ) {
    throw new Error("reconnect did not retain the public player ID");
  }
  if (after.matchId !== before.matchId) {
    throw new Error("reconnect moved the player to another match");
  }
  if (!isDeepStrictEqual(after.health, before.health)) {
    throw new Error("reconnect changed authoritative health");
  }
  if (!isDeepStrictEqual(after.inventory, before.inventory)) {
    throw new Error("reconnect changed or duplicated the match inventory");
  }
  if (!isDeepStrictEqual(after.equipment, before.equipment)) {
    throw new Error("reconnect replaced the fixed loadout");
  }
  if (
    after.health.data.status !== "alive" ||
    after.inventory.frozen ||
    after.deathResult !== null
  ) {
    throw new Error("reconnected player did not resume an active seat");
  }
  if (distance(evidence.beforePosition, evidence.afterPosition) > 0.25) {
    throw new Error("reconnect did not retain the authoritative character");
  }
}

export function assertUniqueTerminalOutcome(
  evidence: UniqueTerminalEvidence,
): void {
  const result = evidence.firstResult;
  if (!isDeepStrictEqual(result, evidence.repeatedResult)) {
    throw new Error("terminal result changed after a repeated resolution tick");
  }
  if (
    result.matchId !== evidence.matchId ||
    result.publicPlayerId !== evidence.victimPlayerId ||
    result.status !== evidence.expectedStatus ||
    result.terminalCause !== evidence.expectedCause ||
    result.killerPublicPlayerId !== evidence.expectedKillerPlayerId
  ) {
    throw new Error("terminal result identity or cause is inconsistent");
  }
  if (
    result.terminalAt === null ||
    result.survivedMs === null ||
    result.settlement !== null
  ) {
    throw new Error("death or timeout result omitted terminal evidence");
  }
  if (
    result.stats.lost.dirt !== evidence.expectedLostDirt ||
    result.stats.lost.gold !== 0 ||
    result.stats.lost.diamond !== 0
  ) {
    throw new Error("terminal result did not freeze the expected inventory");
  }
  if (evidence.observedLootIds.length !== 1) {
    throw new Error("terminal resolution did not create exactly one loot ID");
  }
  const expectedPrefix = `drop:v1:${evidence.matchId}:seat:`;
  const lootId = evidence.observedLootIds[0];
  if (evidence.lootCreationCount !== 1) {
    throw new Error("terminal resolution emitted duplicate loot CREATE frames");
  }
  if (
    !lootId.startsWith(expectedPrefix) ||
    !/^\d+:death$/u.test(lootId.slice(expectedPrefix.length))
  ) {
    throw new Error("terminal loot ID is not the deterministic death ID");
  }
  if (
    evidence.visibleLoot.length !== 1 ||
    evidence.visibleLoot[0].id !== lootId
  ) {
    throw new Error("terminal loot entity is not uniquely visible");
  }
  const metadata = readRecord(
    evidence.visibleLoot[0].metadata,
    "terminal loot metadata",
  );
  const loot = readRecord(metadata.loot, "terminal loot metadata.loot");
  const contents = readRecord(loot.contents, "terminal loot contents");
  if (
    loot.id !== lootId ||
    contents.dirt !== evidence.expectedLostDirt ||
    contents.gold !== 0 ||
    contents.diamond !== 0
  ) {
    throw new Error(
      "terminal loot contents do not conserve the lost inventory",
    );
  }
}

function distance(left: LiveVector3, right: LiveVector3): number {
  return Math.hypot(left[0] - right[0], left[1] - right[1], left[2] - right[2]);
}
