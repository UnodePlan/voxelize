import type { LocalResourceKey } from "./state";
import type { LocalHeldTool } from "./viewmodel";

/**
 * 最佳工具类型（对齐 MC mineable/* 标签）。
 * 当前快捷栏只有 空手 / 铁镐 / 铁剑，无铲无斧。
 */
export type LocalPreferredTool = "none" | "shovel" | "axe" | "pickaxe" | "hoe";

/**
 * 原版工具速度表（Java Breaking wiki / 物品组件 destroy_speed）。
 * 铁工具 = 6；剑通用 = 1.5（仅当剑是该方块的有效加速工具时）。
 */
export const VANILLA_TOOL_SPEED = {
  hand: 1,
  wood: 2,
  stone: 4,
  copper: 5,
  iron: 6,
  diamond: 8,
  netherite: 9,
  gold: 12,
  sword: 1.5,
} as const;

/** 单机可用工具映射到的原版材料等级（铁镐 / 铁剑）。 */
const HELD_TOOL_TIER_SPEED: Readonly<Record<LocalHeldTool, number>> = {
  empty: VANILLA_TOOL_SPEED.hand,
  pickaxe: VANILLA_TOOL_SPEED.iron,
  sword: VANILLA_TOOL_SPEED.sword,
};

export interface VanillaMiningInput {
  /** 方块 hardness；负数不可挖（基岩） */
  hardness: number;
  preferredTool: LocalPreferredTool;
  /**
   * 原版 requiresCorrectToolForDrops：
   * true 时非正确工具 canHarvest=false → 分母 100，且不掉落。
   */
  requiresCorrectToolForDrops: boolean;
}

/**
 * 当前工具是否为该方块的「最佳工具类型」。
 * 严格原版：铁镐只加速 pickaxe 标签；剑不对石/土/木提供最佳工具加速
 *（蛛网/竹等特判本单机未收录）。
 */
export function isBestTool(
  tool: LocalHeldTool,
  preferred: LocalPreferredTool,
): boolean {
  if (tool === "pickaxe" && preferred === "pickaxe") return true;
  // 无铲/斧：empty/sword 对 shovel/axe 均非最佳
  return false;
}

/**
 * 能否以可收获方式破坏（影响 30 vs 100 分母 + 掉落）。
 * - 不要求正确工具：手/任意工具均可收获
 * - 要求镐：仅铁镐
 */
export function canHarvestWith(
  tool: LocalHeldTool,
  profile: Pick<
    VanillaMiningInput,
    "preferredTool" | "requiresCorrectToolForDrops"
  >,
): boolean {
  if (!profile.requiresCorrectToolForDrops) return true;
  if (profile.preferredTool === "pickaxe") return tool === "pickaxe";
  // 要求铲/斧但我们没有对应工具 → 不能收获
  if (
    profile.preferredTool === "shovel" ||
    profile.preferredTool === "axe" ||
    profile.preferredTool === "hoe"
  ) {
    return false;
  }
  return true;
}

/** 原版 destroySpeed（非最佳工具恒为 1）。 */
export function destroySpeed(
  tool: LocalHeldTool,
  preferred: LocalPreferredTool,
): number {
  if (!isBestTool(tool, preferred)) return VANILLA_TOOL_SPEED.hand;
  return HELD_TOOL_TIER_SPEED[tool];
}

/**
 * 严格原版挖掘耗时（毫秒）。
 *
 * 每 tick 进度 = destroySpeed / hardness / (canHarvest ? 30 : 100)
 * ticks = ceil(1 / 进度)；进度 ≥ 1 则瞬间（0ms）
 * 秒 = ticks / 20
 *
 * @see https://minecraft.wiki/w/Breaking
 */
export function miningDurationMs(
  profile: VanillaMiningInput,
  tool: LocalHeldTool,
): number {
  const { hardness } = profile;
  if (!Number.isFinite(hardness) || hardness < 0) {
    // 基岩等：调用方应先 isBlockMineable；此处返回极大值避免误破坏
    return Number.MAX_SAFE_INTEGER;
  }
  if (hardness === 0) return 0;

  const speed = destroySpeed(tool, profile.preferredTool);
  const harvest = canHarvestWith(tool, profile);
  const divisor = harvest ? 30 : 100;
  const progressPerTick = speed / hardness / divisor;
  if (progressPerTick >= 1) return 0;

  const ticks = Math.ceil(1 / progressPerTick);
  return Math.round((ticks / 20) * 1000);
}

/** 破坏完成后是否掉落（原版：不能收获则无掉落）。 */
export function harvestDrop(
  drop: LocalResourceKey | null,
  tool: LocalHeldTool,
  profile: Pick<
    VanillaMiningInput,
    "preferredTool" | "requiresCorrectToolForDrops"
  >,
): LocalResourceKey | null {
  if (drop === null) return null;
  if (!canHarvestWith(tool, profile)) return null;
  return drop;
}

/** 挖掘/破碎音高：工具 + 掉落类型 */
export function digRateFor(
  drop: LocalResourceKey | null,
  tool: LocalHeldTool,
): number {
  let rate = tool === "pickaxe" ? 1 : tool === "sword" ? 1.08 : 0.92;
  if (drop === "dirt") rate *= 1.12;
  else if (drop === "gold") rate *= 0.95;
  else if (drop === "diamond") rate *= 0.82;
  return rate;
}

/** 镐敲击更密，空手/剑稍疏 */
export function digIntervalMs(tool: LocalHeldTool): number {
  if (tool === "pickaxe") return 200;
  if (tool === "sword") return 240;
  return 280;
}
