import type { GameplayStateData } from "../../../../contracts/extraction/v1/typescript";

import type { LiveE2eConfig } from "./config";
import { fetchLiveMatchResult } from "./gameplay-http";
import { prepareGameplayMatch } from "./gameplay-live-actors";
import type { LiveVector3 } from "./gameplay-state";
import type { GameplayTime } from "./gameplay-time";
import {
  ReconnectProtocolActor,
  type ReconnectJoinRejection,
} from "./reconnect-actor";
import { assertRecoveredStatePreserved } from "./reconnect-assertions";
import {
  performNonlethalHits,
  prepareReconnectPair,
  releaseReconnectMatch,
  waitForDetachedSeat,
  waitForReboundSeat,
} from "./reconnect-support";
import {
  runDisconnectedKillScenario,
  runReconnectTimeoutScenario,
  type DisconnectedKillEvidence,
  type ReconnectTimeoutEvidence,
} from "./reconnect-terminal-scenarios";
import { deterministicLiveWallet } from "./siwe";

const MATCH_SIZE = 10;
const KILL_SCENARIO_OFFSET = 20;
const TIMEOUT_SCENARIO_OFFSET = 40;

export interface ReconnectRecoveryEvidence {
  after: GameplayStateData;
  afterPosition: LiveVector3;
  before: GameplayStateData;
  beforePosition: LiveVector3;
  playerId: string;
  takeoverRejection: ReconnectJoinRejection;
}

export interface LiveReconnectAcceptanceEvidence {
  disconnectedKill: DisconnectedKillEvidence;
  recovery: ReconnectRecoveryEvidence;
  timeout: ReconnectTimeoutEvidence;
}

/**
 * 三局共享真实网络契约：近截止恢复、断线被杀、精确超时。
 * 调用者必须提供独占的 e2e-control 服务和数据库。
 */
export async function runLiveReconnectAcceptance(
  config: LiveE2eConfig,
  clock: GameplayTime,
  walletOffsetBase: number,
): Promise<LiveReconnectAcceptanceEvidence> {
  requireWalletRange(walletOffsetBase);
  const recovery = await runReconnectRecoveryScenario(
    config,
    clock,
    walletOffsetBase,
  );
  const disconnectedKill = await runDisconnectedKillScenario(
    config,
    clock,
    walletOffsetBase + KILL_SCENARIO_OFFSET,
  );
  const timeout = await runReconnectTimeoutScenario(
    config,
    clock,
    walletOffsetBase + TIMEOUT_SCENARIO_OFFSET,
  );
  return { disconnectedKill, recovery, timeout };
}

export async function runReconnectRecoveryScenario(
  config: LiveE2eConfig,
  clock: GameplayTime,
  walletOffset: number,
): Promise<ReconnectRecoveryEvidence> {
  const match = await prepareGameplayMatch(config, walletOffset);
  const extraActors: ReconnectProtocolActor[] = [];
  let cleanupPlayer = match.players[0];
  try {
    const pair = await prepareReconnectPair(match, clock);
    cleanupPlayer = pair.attacker;
    await performNonlethalHits(
      pair.attacker.driver,
      pair.victim.driver,
      1,
      clock,
    );
    const before = await pair.victim.driver.getState();
    const beforePosition = pair.victim.driver.position();
    const playerId = pair.victim.driver.playerId;
    await pair.victim.driver.disconnectTransport();
    await waitForDetachedSeat(pair.attacker, 9);

    // 在 60 秒边界前最后 1ms 验证异账号接管失败，再由原账号自动 rebind。
    await clock.elapse(59_999);
    const outsider = new ReconnectProtocolActor(
      `${pair.victim.actorId}-outsider`,
      deterministicLiveWallet(walletOffset + MATCH_SIZE),
      config,
    );
    extraActors.push(outsider);
    const takeoverRejection = await outsider.expectJoinRejected(
      match.worldName,
      playerId,
    );
    await outsider.disconnect();

    const rebound = new ReconnectProtocolActor(
      `${pair.victim.actorId}-rebound`,
      deterministicLiveWallet(walletOffset + pair.victimIndex),
      config,
    );
    extraActors.push(rebound);
    const session = await rebound.automaticRebind(
      match.matchId,
      match.worldName,
    );
    await waitForReboundSeat(pair.attacker);
    await clock.elapse(1);
    const after = await session.driver.getState();
    const afterPosition = session.driver.position();
    assertRecoveredStatePreserved({
      after,
      afterPlayerId: session.driver.playerId,
      afterPosition,
      before,
      beforePlayerId: playerId,
      beforePosition,
    });
    if ((await fetchLiveMatchResult(session.http, match.matchId)) !== null) {
      throw new Error(
        "successful rebind unexpectedly produced a terminal result",
      );
    }
    return {
      after,
      afterPosition,
      before,
      beforePosition,
      playerId,
      takeoverRejection,
    };
  } finally {
    await releaseReconnectMatch(match, cleanupPlayer, extraActors, clock);
  }
}

function requireWalletRange(offset: number): void {
  if (
    !Number.isSafeInteger(offset) ||
    offset < 0 ||
    !Number.isSafeInteger(offset + TIMEOUT_SCENARIO_OFFSET + MATCH_SIZE - 1)
  ) {
    throw new Error("reconnect wallet offset range is invalid");
  }
}
