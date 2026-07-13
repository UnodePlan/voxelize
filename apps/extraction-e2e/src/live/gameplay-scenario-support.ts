import { decodeExtractionStateEnvelope } from "../../../../contracts/extraction/v1/extraction";
import type { ExtractionZoneState } from "../../../../contracts/extraction/v1/typescript";
import type {
  MatchResult,
  WarehouseSnapshot,
} from "../../../extraction-client/src/api/models";

import { moveTogether, pollGameplayState } from "./gameplay-actions";
import type { GameplayProtocolDriver } from "./gameplay-driver";
import { GameplaySocketClosedError } from "./gameplay-errors";
import { fetchLiveMatchResult, fetchLiveWarehouse } from "./gameplay-http";
import { horizontalDistance } from "./gameplay-state";
import type { GameplayTime, GameplayTimeAck } from "./gameplay-time";
import type { ProtocolGameplaySession } from "./protocol-actor";

const HTTP_POLL_TIMEOUT_MS = 30_000;

export interface GameplayScenarioPlayer extends ProtocolGameplaySession {
  actorId: string;
}

export async function moveCombatantsToStaging(
  attacker: GameplayProtocolDriver,
  victim: GameplayProtocolDriver,
  zone: ExtractionZoneState,
  clock: GameplayTime,
): Promise<void> {
  const center = zone.center;
  const stagingX = center[0] + zone.radiusBlocks + 2;
  await moveTogether(
    [
      { driver: attacker, target: [stagingX, center[1], center[2] - 1] },
      { driver: victim, target: [stagingX, center[1], center[2] + 1] },
    ],
    clock,
  );
}

export async function requireInside(
  driver: GameplayProtocolDriver,
): Promise<void> {
  await pollGameplayState(
    driver,
    (state) =>
      state.extraction.data.status === "open" && state.extraction.data.inside,
    "player did not enter the extraction zone",
  );
}

export async function requireOutside(
  driver: GameplayProtocolDriver,
): Promise<void> {
  await pollGameplayState(
    driver,
    (state) =>
      state.extraction.data.status === "open" && !state.extraction.data.inside,
    "player did not remain outside the extraction zone",
  );
}

export async function finishExtraction(
  driver: GameplayProtocolDriver,
  clock: GameplayTime,
): Promise<"pending" | "socketClosed"> {
  const state = await driver.getState();
  if (
    state.extraction.data.status !== "open" ||
    !state.extraction.data.inside
  ) {
    throw new Error("killer left the extraction zone before settlement");
  }
  const remaining =
    state.extraction.data.requiredMs - state.extraction.data.elapsedMs;
  if (remaining <= 0)
    throw new Error("extraction qualified before boundary proof");
  if (remaining > 1) {
    await clock.elapse(remaining - 1);
    const before = await driver.getState();
    if (before.extraction.data.status !== "open") {
      throw new Error(
        "extraction completed before the exact eight-second boundary",
      );
    }
  }
  await clock.elapse(1);
  try {
    await driver.waitForMethodState(
      "pvp:v1:extraction-state",
      (value) => decodeExtractionStateEnvelope(value, driver.manifest),
      (value) => value.data.status === "pending",
      "extraction did not enter settlement pending",
    );
    return "pending";
  } catch (error) {
    // 结算提交后服务端会立即驱逐终态连接；随后由本人 HTTP 结果和仓库快照证明成功。
    if (!(error instanceof GameplaySocketClosedError)) throw error;
    return "socketClosed";
  }
}

export async function advanceTo(
  clock: GameplayTime,
  targetMs: number,
): Promise<void> {
  const current = requireControlledTime(await clock.elapse(0));
  if (current > targetMs)
    throw new Error("controlled clock passed extraction open");
  const reached = requireControlledTime(await clock.elapse(targetMs - current));
  if (reached !== targetMs)
    throw new Error("controlled clock ACK did not reach target");
}

export function requireControlledTime(ack: GameplayTimeAck): number {
  if (ack.monotonicMs === null) {
    throw new Error(
      "full gameplay scenario requires the controlled server clock",
    );
  }
  return ack.monotonicMs;
}

export function nearestPlayers(
  players: readonly GameplayScenarioPlayer[],
): [GameplayScenarioPlayer, GameplayScenarioPlayer] {
  let best: [GameplayScenarioPlayer, GameplayScenarioPlayer] | null = null;
  let bestDistance = Number.POSITIVE_INFINITY;
  for (let left = 0; left < players.length; left += 1) {
    for (let right = left + 1; right < players.length; right += 1) {
      const distance = horizontalDistance(
        players[left].driver.position(),
        players[right].driver.position(),
      );
      if (distance < bestDistance) {
        best = [players[left], players[right]];
        bestDistance = distance;
      }
    }
  }
  if (best === null) throw new Error("no gameplay player pair is available");
  return best;
}

export async function pollMatchResult(
  player: GameplayScenarioPlayer,
  matchId: string,
  status: MatchResult["status"],
): Promise<MatchResult> {
  const result = await pollHttp(
    () => fetchLiveMatchResult(player.http, matchId),
    (value) => value !== null && value.status === status,
    `match result did not reach ${status}`,
  );
  if (result === null)
    throw new Error("match result disappeared after polling");
  return result;
}

export async function pollWarehouse(
  player: GameplayScenarioPlayer,
  predicate: (warehouse: WarehouseSnapshot) => boolean,
): Promise<WarehouseSnapshot> {
  return pollHttp(
    () => fetchLiveWarehouse(player.http),
    predicate,
    "warehouse did not reflect the settlement",
  );
}

export function sameWarehouseSnapshot(
  left: WarehouseSnapshot,
  right: WarehouseSnapshot,
): boolean {
  const resources = Object.keys(left.resources) as Array<
    keyof WarehouseSnapshot["resources"]
  >;
  const stats = Object.keys(left.stats) as Array<
    keyof WarehouseSnapshot["stats"]
  >;
  return (
    resources.every((key) => left.resources[key] === right.resources[key]) &&
    stats.every((key) => left.stats[key] === right.stats[key])
  );
}

async function pollHttp<T>(
  read: () => Promise<T>,
  predicate: (value: T) => boolean,
  message: string,
): Promise<T> {
  const deadline = Date.now() + HTTP_POLL_TIMEOUT_MS;
  for (;;) {
    const value = await read();
    if (predicate(value)) return value;
    if (Date.now() >= deadline) throw new Error(message);
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
}
