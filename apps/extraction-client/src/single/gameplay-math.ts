import type { Vector3 } from "three";

import type { LocalExtractionZone } from "./map";
import type { LocalGameState } from "./state";

/** 低于此高度视为坠落出地形（虚空） */
export const LOCAL_VOID_Y = -2;
/** 随机重降：相对地表抬升高度 */
export const LOCAL_SKY_DROP_HEIGHT = 28;

/**
 * 撤离区竖直容差（相对 zone.center.y = 广场地面）。
 * 玩家 position 是眼睛高度（约地表 +1.6）；跳跃峰值约再 +2～3，
 * 旧阈值 ±4 会在起跳时把人判出圈、进度清零。
 */
export const EXTRACTION_Y_BELOW = 2;
/** 向上足够覆盖连跳/落点，仍排除飞到极高空或其它层 */
export const EXTRACTION_Y_ABOVE = 18;

/**
 * 是否在撤离圆柱内（水平圆 + 宽松竖直）。
 * 水平出圈才中断计时；圈内跳跃不中断。
 */
export function isInsideExtraction(
  position: Vector3,
  zone: LocalExtractionZone,
): boolean {
  const dx = position.x - zone.center[0];
  const dz = position.z - zone.center[2];
  if (dx * dx + dz * dz > zone.radius * zone.radius) return false;
  const dy = position.y - zone.center[1];
  return dy >= -EXTRACTION_Y_BELOW && dy <= EXTRACTION_Y_ABOVE;
}

/** 掉出地图下方虚空，或水平远离地图后坠落 */
export function hasFallenOutOfTerrain(
  position: Pick<Vector3, "x" | "y" | "z">,
  worldMin: number,
  worldMax: number,
  voidY: number = LOCAL_VOID_Y,
): boolean {
  if (position.y < voidY) return true;
  // 飞出水平边界且已经在地表以下：同样按坠落处理
  const outside =
    position.x < worldMin - 1 ||
    position.x > worldMax + 2 ||
    position.z < worldMin - 1 ||
    position.z > worldMax + 2;
  return outside && position.y < 4;
}

/**
 * 在地图范围内随机一点，从地表上方高空落下。
 * @returns 眼睛/相机位置（RigidControls.teleportToExact 用）
 */
export function pickRandomSkyDrop(
  worldMin: number,
  worldMax: number,
  surfaceY: (x: number, z: number) => number,
  options: {
    dropHeight?: number;
    maxY?: number;
    random?: () => number;
  } = {},
): readonly [number, number, number] {
  const dropHeight = options.dropHeight ?? LOCAL_SKY_DROP_HEIGHT;
  const maxY = options.maxY ?? 46;
  const random = options.random ?? Math.random;
  const span = worldMax - worldMin + 1;
  const fx = worldMin + Math.floor(random() * span);
  const fz = worldMin + Math.floor(random() * span);
  const x = Math.min(worldMax, Math.max(worldMin, fx));
  const z = Math.min(worldMax, Math.max(worldMin, fz));
  const ground = surfaceY(x, z);
  const y = Math.min(maxY, ground + dropHeight);
  return [x + 0.5, y, z + 0.5];
}

export function miningProgress(state: LocalGameState): number | null {
  return state.mining === null
    ? null
    : Math.min(1, state.mining.elapsedMs / state.mining.requiredMs);
}
