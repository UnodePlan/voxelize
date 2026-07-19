/**
 * 快捷栏 → 手持内容（工具 / 方块）。
 * 与 combat/mining 用的 LocalHeldTool 分离：资源槽对战斗仍算空手。
 */

import type { LocalInventorySlot, LocalResourceKey } from "./state";

export type LocalHeldContent =
  | { kind: "empty" }
  | { kind: "tool"; tool: "pickaxe" | "sword" }
  | { kind: "block"; resource: LocalResourceKey };

export function heldContentKey(content: LocalHeldContent): string {
  if (content.kind === "empty") return "empty";
  if (content.kind === "tool") return `tool:${content.tool}`;
  return `block:${content.resource}`;
}

export function heldContentEquals(
  a: LocalHeldContent,
  b: LocalHeldContent,
): boolean {
  return heldContentKey(a) === heldContentKey(b);
}

/**
 * 槽 0 空手；1 镐；2 剑；3+ 有数量的资源 → 手持方块。
 */
export function heldContentFromSlot(
  slot: number,
  inventory: ReadonlyArray<LocalInventorySlot | null>,
): LocalHeldContent {
  if (slot === 1) return { kind: "tool", tool: "pickaxe" };
  if (slot === 2) return { kind: "tool", tool: "sword" };
  if (Number.isInteger(slot) && slot >= 3 && slot < inventory.length) {
    const entry = inventory[slot];
    if (entry !== null && entry.quantity > 0) {
      return { kind: "block", resource: entry.resource };
    }
  }
  return { kind: "empty" };
}
