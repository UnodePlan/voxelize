/**
 * 由权威 Gameplay 状态差分推导音效线索（不依赖客户端伪造）。
 */

import type { GameplayStateData } from "../../../../contracts/extraction/v1/typescript";

export type MatchSfxCue =
  | "hitTaken"
  | "death"
  | "drop"
  | "pickup"
  | "kill"
  | "hit";

export function cuesFromGameplayTransition(
  previous: GameplayStateData | null,
  next: GameplayStateData,
): MatchSfxCue[] {
  if (previous === null || previous.matchId !== next.matchId) return [];

  const cues: MatchSfxCue[] = [];

  // 自己死亡：死亡结果首次出现
  if (previous.deathResult === null && next.deathResult !== null) {
    cues.push("death");
    if (tallyTotal(next.deathResult.data.lost) > 0) cues.push("drop");
  }

  // 自己受伤（半心减少且仍存活）
  if (
    previous.health.data.status === "alive" &&
    next.health.data.status === "alive" &&
    next.health.data.currentHalfHearts < previous.health.data.currentHalfHearts
  ) {
    cues.push("hitTaken");
  }

  // 主动丢弃（整槽 Q）
  const prevDrop = previous.inventory.lastDropSequence;
  const nextDrop = next.inventory.lastDropSequence;
  if (
    typeof nextDrop === "number" &&
    (prevDrop === null || nextDrop > prevDrop)
  ) {
    cues.push("drop");
  }

  // 背包净增 → 拾取或挖完入包
  const prevQty = inventoryQuantity(previous);
  const nextQty = inventoryQuantity(next);
  if (nextQty > prevQty) {
    cues.push("pickup");
  }

  return cues;
}

export function cuesFromAttackResolution(
  resolution: "miss" | "hit" | "kill",
): MatchSfxCue[] {
  if (resolution === "miss") return [];
  if (resolution === "hit") return ["hit"];
  return ["kill", "drop"];
}

function inventoryQuantity(state: GameplayStateData): number {
  return state.inventory.slots.reduce(
    (sum, slot) => sum + (slot?.quantity ?? 0),
    0,
  );
}

function tallyTotal(tally: {
  dirt: number;
  gold: number;
  diamond: number;
}): number {
  return tally.dirt + tally.gold + tally.diamond;
}
