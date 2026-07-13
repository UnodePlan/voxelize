import type { AttackResultData } from "../../../../contracts/extraction/v1/typescript";
import type { MatchResult } from "../../../extraction-client/src/api/models";

import type { LiveE2eConfig } from "./config";
import { lookAt, waitForOneVisibleLoot } from "./gameplay-actions";
import { fetchLiveMatchResult } from "./gameplay-http";
import { prepareGameplayMatch } from "./gameplay-live-actors";
import { pollMatchResult } from "./gameplay-scenario-support";
import type { GameplayTime } from "./gameplay-time";
import { ReconnectProtocolActor } from "./reconnect-actor";
import { assertUniqueTerminalOutcome } from "./reconnect-assertions";
import {
  performDetachedLethalHit,
  performNonlethalHits,
  prepareReconnectPair,
  releaseReconnectMatch,
  waitForDetachedSeat,
} from "./reconnect-support";
import { deterministicLiveWallet } from "./siwe";

export interface DisconnectedKillEvidence {
  lateAttack: AttackResultData;
  lootId: string;
  result: MatchResult;
  victimPlayerId: string;
}

export interface ReconnectTimeoutEvidence {
  lateAttack: AttackResultData;
  lateJoinRejection: "MATCH_FULL" | "MATCH_ROSTER_LOCKED";
  lootId: string;
  result: MatchResult;
  victimPlayerId: string;
}

export async function runDisconnectedKillScenario(
  config: LiveE2eConfig,
  clock: GameplayTime,
  walletOffset: number,
): Promise<DisconnectedKillEvidence> {
  const match = await prepareGameplayMatch(config, walletOffset);
  let cleanupPlayer = match.players[0];
  try {
    const pair = await prepareReconnectPair(match, clock);
    cleanupPlayer = pair.attacker;
    await performNonlethalHits(
      pair.attacker.driver,
      pair.victim.driver,
      9,
      clock,
    );
    const victimPlayerId = pair.victim.driver.playerId;
    const victimPosition = pair.victim.driver.position();
    await pair.victim.driver.disconnectTransport();
    await waitForDetachedSeat(pair.attacker, 9);

    await performDetachedLethalHit(pair.attacker.driver, victimPosition, clock);
    const result = await pollMatchResult(pair.victim, match.matchId, "dead");
    const lootId = await waitForOneVisibleLoot(pair.attacker.driver);

    // 原重连截止到达后再次攻击，必须保持原死亡结果和唯一掉落。
    await clock.elapse(60_000);
    await lookAt(pair.attacker.driver, victimPosition, clock);
    const lateAttack = await pair.attacker.driver.attack();
    if (lateAttack.resolution !== "miss") {
      throw new Error("already-dead detached player remained attackable");
    }
    const repeatedResult = await requireResult(pair.victim, match.matchId);
    const observedLootIds = pair.attacker.driver.observedLoot();
    assertUniqueTerminalOutcome({
      expectedCause: "melee",
      expectedKillerPlayerId: pair.attacker.driver.playerId,
      expectedLostDirt: pair.minedDirt,
      expectedStatus: "dead",
      firstResult: result,
      lootCreationCount: pair.attacker.driver.lootCreationCount(lootId),
      matchId: match.matchId,
      observedLootIds,
      repeatedResult,
      victimPlayerId,
      visibleLoot: pair.attacker.driver.visibleLoot(),
    });
    if (observedLootIds[0] !== lootId) {
      throw new Error("melee death loot evidence changed identity");
    }
    return { lateAttack, lootId, result, victimPlayerId };
  } finally {
    await releaseReconnectMatch(match, cleanupPlayer, [], clock);
  }
}

export async function runReconnectTimeoutScenario(
  config: LiveE2eConfig,
  clock: GameplayTime,
  walletOffset: number,
): Promise<ReconnectTimeoutEvidence> {
  const match = await prepareGameplayMatch(config, walletOffset);
  const extraActors: ReconnectProtocolActor[] = [];
  let cleanupPlayer = match.players[0];
  try {
    const pair = await prepareReconnectPair(match, clock);
    cleanupPlayer = pair.attacker;
    const victimPlayerId = pair.victim.driver.playerId;
    const victimPosition = pair.victim.driver.position();
    await pair.victim.driver.disconnectTransport();
    await waitForDetachedSeat(pair.attacker, 9);

    await clock.elapse(59_999);
    if (
      (await fetchLiveMatchResult(pair.victim.http, match.matchId)) !== null
    ) {
      throw new Error(
        "reconnect timeout resolved before the 60-second boundary",
      );
    }
    await clock.elapse(1);
    const result = await pollMatchResult(
      pair.victim,
      match.matchId,
      "timedOut",
    );
    const lootId = await waitForOneVisibleLoot(pair.attacker.driver);

    const lateActor = new ReconnectProtocolActor(
      `${pair.victim.actorId}-late-reconnect`,
      deterministicLiveWallet(walletOffset + pair.victimIndex),
      config,
    );
    extraActors.push(lateActor);
    const lateJoinRejection = await lateActor.expectJoinRejected(
      match.worldName,
      victimPlayerId,
    );
    await lateActor.disconnect();

    await lookAt(pair.attacker.driver, victimPosition, clock);
    const lateAttack = await pair.attacker.driver.attack();
    if (lateAttack.resolution !== "miss") {
      throw new Error("timed-out player remained attackable");
    }
    await clock.elapse(1);
    const repeatedResult = await requireResult(
      { http: lateActor.authenticatedHttp() },
      match.matchId,
    );
    const observedLootIds = pair.attacker.driver.observedLoot();
    assertUniqueTerminalOutcome({
      expectedCause: "reconnectTimeout",
      expectedKillerPlayerId: null,
      expectedLostDirt: pair.minedDirt,
      expectedStatus: "timedOut",
      firstResult: result,
      lootCreationCount: pair.attacker.driver.lootCreationCount(lootId),
      matchId: match.matchId,
      observedLootIds,
      repeatedResult,
      victimPlayerId,
      visibleLoot: pair.attacker.driver.visibleLoot(),
    });
    if (observedLootIds[0] !== lootId) {
      throw new Error("timeout loot evidence changed identity");
    }
    return {
      lateAttack,
      lateJoinRejection,
      lootId,
      result,
      victimPlayerId,
    };
  } finally {
    await releaseReconnectMatch(match, cleanupPlayer, extraActors, clock);
  }
}

async function requireResult(
  player: { http: Parameters<typeof fetchLiveMatchResult>[0] },
  matchId: string,
): Promise<MatchResult> {
  const result = await fetchLiveMatchResult(player.http, matchId);
  if (result === null) throw new Error("terminal result disappeared");
  return result;
}
