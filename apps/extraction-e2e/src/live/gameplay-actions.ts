import type { GameplayStateData } from "../../../../contracts/extraction/v1/typescript";

import { GameplayProtocolDriver } from "./gameplay-driver";
import {
  directionBetween,
  horizontalDistance,
  inventoryResourceCount,
  supportedSurfaceVoxelTowardCenter,
  visibleTopFacePoint,
  type LiveVector3,
} from "./gameplay-state";
import type { GameplayTime } from "./gameplay-time";

const MOVEMENT_STEP_MS = 200;
const ACTION_TIMEOUT_MS = 30_000;
const STABLE_POSITION_SAMPLES = 3;

export interface DirtMiningEvidence {
  after: number;
  before: number;
  target: LiveVector3;
}

export interface MovementDestination {
  driver: GameplayProtocolDriver;
  target: LiveVector3;
}

export async function moveToHorizontal(
  driver: GameplayProtocolDriver,
  target: LiveVector3,
  clock: GameplayTime,
  stopDistance = 0.25,
): Promise<LiveVector3> {
  const deadline = Date.now() + ACTION_TIMEOUT_MS;
  let stagnantSteps = 0;
  for (;;) {
    const before = driver.position();
    const remaining = horizontalDistance(before, target);
    if (remaining <= stopDistance) {
      await stopMovement(driver, target, clock);
      return driver.position();
    }
    if (Date.now() >= deadline)
      throw new Error("authoritative movement timed out");
    const direction = horizontalDirection(before, target);
    await driver.sendMovement({
      direction,
      movement: {
        forward: Math.min(1, remaining),
        jump: stagnantSteps > 0,
        right: 0,
      },
    });
    await clock.elapse(MOVEMENT_STEP_MS);
    const after = driver.position();
    stagnantSteps =
      horizontalDistance(before, after) <= 0.001 ? stagnantSteps + 1 : 0;
    if (stagnantSteps >= 10) {
      throw new Error("authoritative movement made no progress");
    }
  }
}

export async function moveTogether(
  destinations: readonly MovementDestination[],
  clock: GameplayTime,
  stopDistance = 0.25,
): Promise<LiveVector3[]> {
  if (destinations.length < 2) {
    throw new Error("coordinated movement requires at least two players");
  }
  const deadline = Date.now() + ACTION_TIMEOUT_MS * 2;
  const stagnant = destinations.map(() => 0);
  for (;;) {
    const before = destinations.map(({ driver }) => driver.position());
    const remaining = before.map((position, index) =>
      horizontalDistance(position, destinations[index].target),
    );
    if (remaining.every((distance) => distance <= stopDistance)) {
      await Promise.all(
        destinations.map(({ driver, target }) =>
          sendStoppedMovement(driver, target),
        ),
      );
      await clock.elapse(50);
      return destinations.map(({ driver }) => driver.position());
    }
    if (Date.now() >= deadline)
      throw new Error("coordinated movement timed out");
    await Promise.all(
      destinations.map(({ driver, target }, index) => {
        const direction =
          remaining[index] <= Number.EPSILON
            ? ([1, 0, 0] as LiveVector3)
            : horizontalDirection(before[index], target);
        return driver.sendMovement({
          direction,
          movement: {
            forward:
              remaining[index] <= stopDistance
                ? 0
                : Math.min(1, remaining[index]),
            // 挖掉脚下表层后玩家可能落入一格坑；一次停滞后由权威物理尝试跳出。
            jump: stagnant[index] > 0,
            right: 0,
          },
        });
      }),
    );
    await clock.elapse(MOVEMENT_STEP_MS);
    destinations.forEach(({ driver }, index) => {
      const moved = horizontalDistance(before[index], driver.position());
      stagnant[index] = moved <= 0.001 ? stagnant[index] + 1 : 0;
      if (remaining[index] > stopDistance && stagnant[index] >= 10) {
        throw new Error("coordinated authoritative movement made no progress");
      }
    });
  }
}

export async function lookAt(
  driver: GameplayProtocolDriver,
  target: LiveVector3,
  clock: GameplayTime,
): Promise<void> {
  const deadline = Date.now() + ACTION_TIMEOUT_MS;
  let stableSamples = 0;
  for (;;) {
    const origin = driver.position();
    const expected = directionBetween(origin, target);
    // WebSocket send 完成不代表 World actor 已消费 PEER；等后续权威广播确认。
    const acknowledgedPosition = await driver.sendMovementAndWaitForDirection(
      {
        direction: expected,
        movement: { forward: 0, jump: false, right: 0 },
      },
      (direction) => vectorDistanceSquared(direction, expected) <= 0.000001,
      "authoritative look direction was not acknowledged",
    );
    if (vectorDistanceSquared(origin, acknowledgedPosition) <= 0.000001) {
      stableSamples += 1;
      if (stableSamples >= STABLE_POSITION_SAMPLES) break;
    } else {
      stableSamples = 0;
    }
    // 连续多帧确认落稳；每帧都用最新眼睛位置重新计算射线。
    if (Date.now() >= deadline) {
      throw new Error("authoritative look origin did not stabilize");
    }
  }
  await clock.elapse(50);
}

export async function mineOneSurfaceDirt(
  driver: GameplayProtocolDriver,
  clock: GameplayTime,
): Promise<DirtMiningEvidence> {
  const beforeState = await driver.getState();
  const before = inventoryResourceCount(beforeState, "dirt");
  const target = supportedSurfaceVoxelTowardCenter(driver.position());
  await lookAt(driver, visibleTopFacePoint(target), clock);
  await driver.mining({ action: "start", voxel: target });
  for (const advanceMs of [200, 200]) {
    await clock.elapse(advanceMs);
    await driver.mining({ action: "maintain" });
  }
  // 500ms 边界的 world tick 会直接完成泥土挖掘，完成后不能再 maintain。
  await clock.elapse(100);
  const completed = await pollGameplayState(
    driver,
    (state) => inventoryResourceCount(state, "dirt") === before + 1,
    "surface dirt did not enter the authoritative inventory",
  );
  return {
    after: inventoryResourceCount(completed, "dirt"),
    before,
    target,
  };
}

export async function pollGameplayState(
  driver: GameplayProtocolDriver,
  predicate: (state: GameplayStateData) => boolean,
  message: string,
): Promise<GameplayStateData> {
  const deadline = Date.now() + ACTION_TIMEOUT_MS;
  for (;;) {
    const state = await driver.getState();
    if (predicate(state)) return state;
    if (Date.now() >= deadline) throw new Error(message);
    await wallDelay(25);
  }
}

export async function waitForOneVisibleLoot(
  driver: GameplayProtocolDriver,
): Promise<string> {
  const deadline = Date.now() + ACTION_TIMEOUT_MS;
  for (;;) {
    const loot = driver.visibleLoot();
    if (loot.length === 1) return loot[0].id;
    if (loot.length > 1)
      throw new Error("death produced multiple visible loot entities");
    if (Date.now() >= deadline) throw new Error("death loot was not projected");
    await wallDelay(25);
  }
}

async function stopMovement(
  driver: GameplayProtocolDriver,
  target: LiveVector3,
  clock: GameplayTime,
): Promise<void> {
  await sendStoppedMovement(driver, target);
  await clock.elapse(50);
}

function sendStoppedMovement(
  driver: GameplayProtocolDriver,
  target: LiveVector3,
): Promise<void> {
  const current = driver.position();
  const direction =
    horizontalDistance(current, target) <= Number.EPSILON
      ? ([1, 0, 0] as LiveVector3)
      : horizontalDirection(current, target);
  return driver.sendMovement({
    direction,
    movement: { forward: 0, jump: false, right: 0 },
  });
}

function horizontalDirection(from: LiveVector3, to: LiveVector3): LiveVector3 {
  const dx = to[0] - from[0];
  const dz = to[2] - from[2];
  const length = Math.hypot(dx, dz);
  if (!Number.isFinite(length) || length <= Number.EPSILON) {
    throw new Error("horizontal target must differ from player position");
  }
  return [dx / length, 0, dz / length];
}

function vectorDistanceSquared(left: LiveVector3, right: LiveVector3): number {
  return left.reduce((sum, value, index) => {
    const delta = value - right[index];
    return sum + delta * delta;
  }, 0);
}

function wallDelay(milliseconds: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
