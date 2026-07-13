import type { AttackResultData } from "../../../../contracts/extraction/v1/typescript";

import { lookAt, mineOneSurfaceDirt, moveTogether } from "./gameplay-actions";
import type { GameplayProtocolDriver } from "./gameplay-driver";
import type { PreparedGameplayMatch } from "./gameplay-live-actors";
import {
  nearestPlayers,
  type GameplayScenarioPlayer,
} from "./gameplay-scenario-support";
import { horizontalDistance, type LiveVector3 } from "./gameplay-state";
import type { GameplayTime } from "./gameplay-time";
import {
  fetchLiveResourceSnapshot,
  pollUntilFullyReleased,
  type LiveResourceSnapshot,
} from "./resource-snapshot";

const MELEE_COOLDOWN_MS = 600;
const COMBAT_HALF_SEPARATION = 1.3;
const RESOURCE_POLL_TIMEOUT_MS = 30_000;
const RESOURCE_RELEASE_TIMEOUT_MS = 60_000;

export interface PreparedReconnectPair {
  attacker: GameplayScenarioPlayer;
  minedDirt: number;
  victim: GameplayScenarioPlayer;
  victimIndex: number;
}

export interface DisconnectableActor {
  disconnect(): Promise<void>;
}

export async function prepareReconnectPair(
  match: PreparedGameplayMatch,
  clock: GameplayTime,
): Promise<PreparedReconnectPair> {
  const [attacker, victim] = nearestPlayers(match.players);
  const mined = await mineOneSurfaceDirt(victim.driver, clock);
  if (mined.before !== 0 || mined.after !== 1) {
    throw new Error("reconnect victim did not start empty and mine one dirt");
  }
  await stageCombatants(attacker.driver, victim.driver, clock);
  const victimIndex = match.players.indexOf(victim);
  if (victimIndex < 0) throw new Error("reconnect victim is not in the roster");
  return { attacker, minedDirt: mined.after, victim, victimIndex };
}

export async function performNonlethalHits(
  attacker: GameplayProtocolDriver,
  victim: GameplayProtocolDriver,
  count: number,
  clock: GameplayTime,
): Promise<AttackResultData[]> {
  if (!Number.isSafeInteger(count) || count < 1 || count > 9) {
    throw new Error("nonlethal hit count must be in 1..9");
  }
  const results: AttackResultData[] = [];
  await lookAt(victim, attacker.position(), clock);
  for (let index = 0; index < count; index += 1) {
    if (index > 0) await clock.elapse(MELEE_COOLDOWN_MS);
    await lookAt(attacker, victim.position(), clock);
    const result = await attacker.attack();
    const current = (await victim.getState()).health.data.currentHalfHearts;
    if (result.resolution !== "hit" || current !== 18 - index * 2) {
      throw new Error("nonlethal reconnect setup produced invalid damage");
    }
    results.push(result);
  }
  return results;
}

export async function performDetachedLethalHit(
  attacker: GameplayProtocolDriver,
  victimPosition: LiveVector3,
  clock: GameplayTime,
): Promise<AttackResultData> {
  await clock.elapse(MELEE_COOLDOWN_MS);
  await lookAt(attacker, victimPosition, clock);
  const result = await attacker.attack();
  if (result.resolution !== "kill") {
    throw new Error("detached victim was not killed by the tenth hit");
  }
  return result;
}

export function waitForDetachedSeat(
  player: GameplayScenarioPlayer,
  expectedAttached: number,
): Promise<LiveResourceSnapshot> {
  return pollResources(
    player,
    (snapshot) =>
      snapshot.server.connections === expectedAttached &&
      snapshot.server.detachedConnections === 1 &&
      snapshot.worlds.length === 1 &&
      snapshot.worlds[0].clientCount === 10,
    "disconnected player did not enter one retained detached seat",
  );
}

export function waitForReboundSeat(
  player: GameplayScenarioPlayer,
): Promise<LiveResourceSnapshot> {
  return pollResources(
    player,
    (snapshot) =>
      snapshot.server.connections === 10 &&
      snapshot.server.detachedConnections === 0 &&
      snapshot.server.pendingRebinds === 0,
    "same-account socket did not complete the automatic rebind",
  );
}

export async function releaseReconnectMatch(
  match: PreparedGameplayMatch,
  cleanupTransport: GameplayScenarioPlayer,
  extraActors: readonly DisconnectableActor[],
  clock: GameplayTime,
): Promise<void> {
  const closed = await Promise.allSettled([
    ...extraActors.map((actor) => actor.disconnect()),
    match.disconnect(),
  ]);
  if (closed.some((result) => result.status === "rejected")) {
    throw new Error("one or more reconnect actors failed to disconnect");
  }
  await pollResources(
    cleanupTransport,
    (snapshot) =>
      snapshot.server.connections === 0 &&
      snapshot.server.lostSessions === 0 &&
      snapshot.server.pendingDetaches === 0 &&
      snapshot.server.pendingRebinds === 0 &&
      snapshot.coordinator.connectedAccounts === 0,
    "reconnect sockets did not finish detaching before cleanup",
  );
  await clock.elapse(60_000);
  await clock.elapse(0);
  await pollUntilFullyReleased(
    cleanupTransport.http,
    RESOURCE_RELEASE_TIMEOUT_MS,
    "reconnect match cleanup",
  );
}

async function stageCombatants(
  attacker: GameplayProtocolDriver,
  victim: GameplayProtocolDriver,
  clock: GameplayTime,
): Promise<void> {
  const left = attacker.position();
  const right = victim.position();
  const dx = right[0] - left[0];
  const dz = right[2] - left[2];
  const length = Math.hypot(dx, dz);
  if (!Number.isFinite(length) || length <= Number.EPSILON) {
    throw new Error("reconnect combatants started at the same position");
  }
  const unitX = dx / length;
  const unitZ = dz / length;
  const center: LiveVector3 = [
    (left[0] + right[0]) / 2,
    Math.min(left[1], right[1]),
    (left[2] + right[2]) / 2,
  ];
  await moveTogether(
    [
      {
        driver: attacker,
        target: [
          center[0] - unitX * COMBAT_HALF_SEPARATION,
          center[1],
          center[2] - unitZ * COMBAT_HALF_SEPARATION,
        ],
      },
      {
        driver: victim,
        target: [
          center[0] + unitX * COMBAT_HALF_SEPARATION,
          center[1],
          center[2] + unitZ * COMBAT_HALF_SEPARATION,
        ],
      },
    ],
    clock,
  );
  const separation = horizontalDistance(attacker.position(), victim.position());
  if (separation <= 2 || separation > 3.1) {
    throw new Error("reconnect combat staging is outside the safe melee band");
  }
}

async function pollResources(
  player: GameplayScenarioPlayer,
  predicate: (snapshot: LiveResourceSnapshot) => boolean,
  message: string,
): Promise<LiveResourceSnapshot> {
  const deadline = Date.now() + RESOURCE_POLL_TIMEOUT_MS;
  for (;;) {
    const snapshot = await fetchLiveResourceSnapshot(player.http);
    if (predicate(snapshot)) return snapshot;
    if (Date.now() >= deadline) {
      throw new Error(`${message}: ${JSON.stringify(snapshot)}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
}
