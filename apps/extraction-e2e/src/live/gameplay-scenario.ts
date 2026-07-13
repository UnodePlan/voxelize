import {
  decodeDeathResultEnvelope,
  decodeHealthStateEnvelope,
} from "../../../../contracts/extraction/v1/combat";
import type {
  AttackResultData,
  DeathResultEnvelope,
  ExtractionZoneState,
} from "../../../../contracts/extraction/v1/typescript";
import type {
  MatchResult,
  WarehouseSnapshot,
} from "../../../extraction-client/src/api/models";

import {
  lookAt,
  mineOneSurfaceDirt,
  moveToHorizontal,
  pollGameplayState,
  waitForOneVisibleLoot,
  type DirtMiningEvidence,
} from "./gameplay-actions";
import type { GameplayProtocolDriver } from "./gameplay-driver";
import { fetchLiveWarehouse } from "./gameplay-http";
import {
  advanceTo,
  finishExtraction,
  moveCombatantsToStaging,
  nearestPlayers,
  pollMatchResult,
  pollWarehouse,
  requireControlledTime,
  requireInside,
  requireOutside,
  sameWarehouseSnapshot,
  type GameplayScenarioPlayer,
} from "./gameplay-scenario-support";
import { inventoryResourceCount } from "./gameplay-state";
import type { GameplayTime } from "./gameplay-time";

const EXTRACTION_OPENS_AFTER_MS = 480_000;
const MELEE_COOLDOWN_MS = 600;
export type { GameplayScenarioPlayer } from "./gameplay-scenario-support";

export interface GameplayScenarioOptions {
  assertTenSockets(): Promise<void>;
  clock: GameplayTime;
  matchId: string;
  players: readonly GameplayScenarioPlayer[];
}

export interface HitEvidence extends AttackResultData {
  victimHalfHearts: number;
}

export interface GameplayScenarioEvidence {
  attackerId: string;
  attackerResult: MatchResult;
  attacks: HitEvidence[];
  death: DeathResultEnvelope;
  dirt: DirtMiningEvidence;
  extractionZone: ExtractionZoneState;
  lootId: string;
  matchStartMonotonicMs: number;
  pickup: { after: number; before: number };
  victimId: string;
  victimResult: MatchResult;
  warehouses: {
    attackerAfter: WarehouseSnapshot;
    attackerBefore: WarehouseSnapshot;
    attackerRecheck: WarehouseSnapshot;
    victimAfter: WarehouseSnapshot;
    victimBefore: WarehouseSnapshot;
  };
}

export async function runLiveGameplayScenario(
  options: GameplayScenarioOptions,
): Promise<GameplayScenarioEvidence> {
  if (options.players.length < 2) {
    throw new Error("live gameplay scenario requires two protocol players");
  }
  await options.assertTenSockets();
  const matchStartMonotonicMs = requireControlledTime(
    await options.clock.elapse(0),
  );
  const [attacker, victim] = nearestPlayers(options.players);
  const [attackerBefore, victimBefore] = await Promise.all([
    fetchLiveWarehouse(attacker.http),
    fetchLiveWarehouse(victim.http),
  ]);
  const dirt = await mineOneSurfaceDirt(victim.driver, options.clock);
  if (dirt.after - dirt.before !== 1) {
    throw new Error("surface mining did not produce exactly one dirt");
  }
  await advanceTo(
    options.clock,
    matchStartMonotonicMs + EXTRACTION_OPENS_AFTER_MS,
  );
  const open = await pollGameplayState(
    attacker.driver,
    (state) => state.extraction.data.status === "open",
    "extraction phase did not open at eight minutes",
  );
  if (open.extraction.data.status !== "open") throw new Error("unreachable");
  const extractionZone = open.extraction.data.zone;
  await moveCombatantsToStaging(
    attacker.driver,
    victim.driver,
    extractionZone,
    options.clock,
  );
  await Promise.all([
    requireOutside(attacker.driver),
    requireOutside(victim.driver),
  ]);
  await options.assertTenSockets();

  const pickupBefore = inventoryResourceCount(
    await attacker.driver.getState(),
    "dirt",
  );
  const victimPosition = victim.driver.position();
  const attacks = await performTenAttacks(
    attacker.driver,
    victim.driver,
    options.clock,
  );
  const death = await victim.driver.waitForMethodState(
    "pvp:v1:death-result",
    (value) => decodeDeathResultEnvelope(value, victim.driver.manifest),
    (value) => value.data.cause === "melee",
    "victim did not receive the melee death result",
  );
  if (death.data.lost.dirt !== dirt.after) {
    throw new Error("death result did not freeze the mined dirt");
  }
  const lootId = await waitForOneVisibleLoot(attacker.driver);
  await moveToHorizontal(attacker.driver, victimPosition, options.clock, 1.2);
  const pickedUp = await pollGameplayState(
    attacker.driver,
    (state) =>
      inventoryResourceCount(state, "dirt") ===
      pickupBefore + death.data.lost.dirt,
    "another player did not automatically pick up the death loot",
  );
  const pickupAfter = inventoryResourceCount(pickedUp, "dirt");
  if (attacker.driver.visibleLoot().some(({ id }) => id === lootId)) {
    throw new Error("fully picked death loot remained visible");
  }
  if (attacker.driver.lootCreationCount(lootId) !== 1) {
    throw new Error("death loot emitted more than one CREATE frame");
  }

  await moveToHorizontal(
    attacker.driver,
    extractionZone.center,
    options.clock,
    1.2,
  );
  await requireInside(attacker.driver);
  await finishExtraction(attacker.driver, options.clock);
  const [attackerResult, victimResult] = await Promise.all([
    pollMatchResult(attacker, options.matchId, "extracted"),
    pollMatchResult(victim, options.matchId, "dead"),
  ]);
  const attackerAfter = await pollWarehouse(
    attacker,
    (warehouse) =>
      warehouse.resources.dirt ===
        attackerBefore.resources.dirt + pickupAfter &&
      warehouse.stats.successfulExtractions ===
        attackerBefore.stats.successfulExtractions + 1,
  );
  const victimAfter = await fetchLiveWarehouse(victim.http);
  if (!sameWarehouseSnapshot(victimAfter, victimBefore)) {
    throw new Error("dead player's warehouse or aggregate stats changed");
  }
  await options.clock.elapse(0);
  const attackerRecheck = await fetchLiveWarehouse(attacker.http);
  if (!sameWarehouseSnapshot(attackerRecheck, attackerAfter)) {
    throw new Error("settlement was applied more than once");
  }
  const uniqueLootQuantity = death.data.lost.dirt;
  assertWarehouseSettlement(attackerBefore, attackerAfter, uniqueLootQuantity);
  if (
    pickupAfter - pickupBefore !== uniqueLootQuantity ||
    !sameResourceCounts(attackerResult.stats.pickedUp, death.data.lost) ||
    !sameResourceCounts(victimResult.stats.lost, death.data.lost) ||
    !sameResourceCounts(attackerResult.settlement?.resources, death.data.lost)
  ) {
    throw new Error("settlement and warehouse evidence disagree");
  }
  return {
    attackerId: attacker.actorId,
    attackerResult,
    attacks,
    death,
    dirt,
    extractionZone,
    lootId,
    matchStartMonotonicMs,
    pickup: { after: pickupAfter, before: pickupBefore },
    victimId: victim.actorId,
    victimResult,
    warehouses: {
      attackerAfter,
      attackerBefore,
      attackerRecheck,
      victimAfter,
      victimBefore,
    },
  };
}

async function performTenAttacks(
  attacker: GameplayProtocolDriver,
  victim: GameplayProtocolDriver,
  clock: GameplayTime,
): Promise<HitEvidence[]> {
  const evidence: HitEvidence[] = [];
  // 水平到位时角色仍可能处于脱困跳跃；先确认受击者连续多帧落稳。
  await lookAt(victim, attacker.position(), clock);
  for (let index = 0; index < 10; index += 1) {
    if (index > 0) await clock.elapse(MELEE_COOLDOWN_MS);
    // 恢复跳跃或重力落地会改变眼睛高度；每次挥击前按最新权威位置重新瞄准。
    await lookAt(attacker, victim.position(), clock);
    const result = await attacker.attack();
    const expectedHalfHearts = 18 - index * 2;
    const health = await victim.waitForMethodState(
      "pvp:v1:health-state",
      (value) => decodeHealthStateEnvelope(value, victim.manifest),
      (value) => value.data.currentHalfHearts === expectedHalfHearts,
      `victim health did not reach ${expectedHalfHearts} half-hearts`,
    );
    const victimHalfHearts = health.data.currentHalfHearts;
    evidence.push({ ...result, victimHalfHearts });
  }
  const expected = evidence.map((_, index) => (index === 9 ? "kill" : "hit"));
  if (evidence.some((hit, index) => hit.resolution !== expected[index])) {
    throw new Error(
      `ten melee attacks did not resolve as nine hits and one kill: ${JSON.stringify(
        evidence.map(({ resolution, victimHalfHearts }) => ({
          resolution,
          victimHalfHearts,
        })),
      )}`,
    );
  }
  if (evidence.some((hit, index) => hit.victimHalfHearts !== 18 - index * 2)) {
    throw new Error(
      "authoritative health did not lose two half-hearts per hit",
    );
  }
  return evidence;
}

function assertWarehouseSettlement(
  before: WarehouseSnapshot,
  after: WarehouseSnapshot,
  dirt: number,
): void {
  const valid =
    after.resources.dirt === before.resources.dirt + dirt &&
    after.resources.gold === before.resources.gold &&
    after.resources.diamond === before.resources.diamond &&
    after.stats.totalResourcesExtracted ===
      before.stats.totalResourcesExtracted + dirt &&
    after.stats.totalExtractionValue ===
      before.stats.totalExtractionValue + dirt &&
    after.stats.successfulExtractions ===
      before.stats.successfulExtractions + 1 &&
    after.stats.highestSingleMatchValue ===
      Math.max(before.stats.highestSingleMatchValue, dirt);
  if (!valid) throw new Error("warehouse aggregate settlement is inconsistent");
}

function sameResourceCounts(
  left: { dirt: number; gold: number; diamond: number } | undefined,
  right: { dirt: number; gold: number; diamond: number },
): boolean {
  return (
    left !== undefined &&
    left.dirt === right.dirt &&
    left.gold === right.gold &&
    left.diamond === right.diamond
  );
}
