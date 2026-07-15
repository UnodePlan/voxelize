/**
 * 单机生命 / 近战（对齐原版 MC 心心刻度 + 生产近战参数）。
 *
 * - 1 颗星（心）= 2 半心；满血 10 星 = 20 半心
 * - 空手/镐攻击 = 半星（1 半心）
 * - 铁剑攻击 = 1 整星（2 半心）
 * - 触及 / 冷却对齐生产 MELEE_REACH=3、约 600ms 节奏
 * - 击退：生产无；单机用冲量（玩家 RigidBody / 假人运动学速度）
 */

import type { LocalHeldTool } from "./viewmodel";

/** 满血心心数（显示用） */
export const LOCAL_MAX_HEARTS = 10;
/** 半心为单位的最大生命（MC 同款刻度） */
export const LOCAL_MAX_HEALTH = LOCAL_MAX_HEARTS * 2;
/** 近战触及距离（格）；对齐生产 MELEE_REACH */
export const LOCAL_ATTACK_RANGE = 3.0;
/** 攻击冷却（ms）；对齐生产连砍节奏 */
export const LOCAL_ATTACK_COOLDOWN_MS = 600;

/**
 * 水平击退冲量（mass≈1 时 ≈ 初速度格/秒）。
 * 剑更强；空手/镐同档。
 */
export const KNOCKBACK_HORIZONTAL: Readonly<Record<LocalHeldTool, number>> = {
  empty: 4,
  pickaxe: 4,
  sword: 7,
};
/** 受击微抬升，避免贴地摩擦立刻刹停 */
export const KNOCKBACK_UP = 2.5;

export type HeartIcon = "full" | "half" | "empty";

export function fullHealth(): number {
  return LOCAL_MAX_HEALTH;
}

/** 当前工具单次命中伤害（半心） */
export function attackDamageHalfHearts(tool: LocalHeldTool): number {
  if (tool === "sword") return 2; // 一颗星
  // 空手、镐：半颗星
  return 1;
}

/**
 * 视线方向 → 击退冲量（世界坐标）。
 * 水平分量沿 look 的 xz 投影；竖直固定微抬。
 */
export function knockbackImpulse(
  tool: LocalHeldTool,
  lookDir: readonly [number, number, number],
): [number, number, number] {
  const horizontal = KNOCKBACK_HORIZONTAL[tool] ?? KNOCKBACK_HORIZONTAL.empty;
  const hx = lookDir[0];
  const hz = lookDir[2];
  const hLen = Math.hypot(hx, hz);
  if (!(hLen > 1e-8)) {
    // 纯俯仰：默认朝 -Z 推
    return [0, KNOCKBACK_UP, -horizontal];
  }
  const scale = horizontal / hLen;
  return [hx * scale, KNOCKBACK_UP, hz * scale];
}

export function applyDamage(health: number, damage: number): number {
  if (!Number.isFinite(health) || !Number.isFinite(damage) || damage <= 0) {
    return clampHealth(health);
  }
  return clampHealth(health - damage);
}

export function clampHealth(health: number): number {
  if (!Number.isFinite(health)) return 0;
  return Math.max(0, Math.min(LOCAL_MAX_HEALTH, Math.floor(health)));
}

export function isDead(health: number): boolean {
  return clampHealth(health) <= 0;
}

/** 10 颗心：满 / 半 / 空（从左到右） */
export function heartsFromHealth(health: number): HeartIcon[] {
  const h = clampHealth(health);
  const icons: HeartIcon[] = [];
  for (let i = 0; i < LOCAL_MAX_HEARTS; i += 1) {
    const remaining = h - i * 2;
    if (remaining >= 2) icons.push("full");
    else if (remaining === 1) icons.push("half");
    else icons.push("empty");
  }
  return icons;
}

export interface Aabb3 {
  minX: number;
  minY: number;
  minZ: number;
  maxX: number;
  maxY: number;
  maxZ: number;
}

/**
 * 从眼睛位置 + 视线方向检测是否命中 AABB。
 * @returns 命中距离；未命中为 null
 */
export function raycastAabb(
  origin: readonly [number, number, number],
  direction: readonly [number, number, number],
  box: Aabb3,
  maxDistance: number,
): number | null {
  const len = Math.hypot(direction[0], direction[1], direction[2]);
  if (!(len > 1e-8) || !(maxDistance > 0)) return null;
  const dx = direction[0] / len;
  const dy = direction[1] / len;
  const dz = direction[2] / len;

  let tMin = 0;
  let tMax = maxDistance;
  const slabs: Array<readonly [number, number, number, number]> = [
    [origin[0], dx, box.minX, box.maxX],
    [origin[1], dy, box.minY, box.maxY],
    [origin[2], dz, box.minZ, box.maxZ],
  ];

  for (const [o, d, minB, maxB] of slabs) {
    if (Math.abs(d) < 1e-12) {
      if (o < minB || o > maxB) return null;
      continue;
    }
    const inv = 1 / d;
    let t0 = (minB - o) * inv;
    let t1 = (maxB - o) * inv;
    if (t0 > t1) {
      const swap = t0;
      t0 = t1;
      t1 = swap;
    }
    tMin = Math.max(tMin, t0);
    tMax = Math.min(tMax, t1);
    if (tMin > tMax) return null;
  }

  if (tMax < 0) return null;
  const hit = tMin >= 0 ? tMin : 0;
  return hit <= maxDistance ? hit : null;
}

/**
 * 人形脚底 + 眼高 → 碰撞盒（略窄于体宽，便于瞄准）。
 * root 原点在眼睛高度时：feetY = eyeY - eyeHeight。
 */
export function humanoidAabb(
  eyePosition: readonly [number, number, number],
  eyeHeight: number,
  options: { halfWidth?: number; bodyHeight?: number } = {},
): Aabb3 {
  const halfW = options.halfWidth ?? 0.3;
  const bodyH = options.bodyHeight ?? eyeHeight / 0.9;
  const feetY = eyePosition[1] - eyeHeight;
  return {
    minX: eyePosition[0] - halfW,
    maxX: eyePosition[0] + halfW,
    minY: feetY,
    maxY: feetY + bodyH,
    minZ: eyePosition[2] - halfW,
    maxZ: eyePosition[2] + halfW,
  };
}
