/**
 * 右键放置一帧逻辑（纯判定 + 副作用委托）。
 * 从 SinglePlayerController.tryPlace 抽出，便于测试与控文件体积。
 */

import { placeBlockIdForResource } from "./blocks";
import {
  consumeInventorySlot,
  isToolHotbarSlot,
  voxelKey,
  type LocalInventorySlot,
  type LocalResourceKey,
} from "./state";

export const LOCAL_PLACE_COOLDOWN_MS = 250;

export interface PlaceAttemptInput {
  pointerLocked: boolean;
  secondaryHeld: boolean;
  wasSecondaryHeld: boolean;
  now: number;
  placeCooldownUntil: number;
  selectedSlot: number;
  inventory: ReadonlyArray<LocalInventorySlot | null>;
  potential: [number, number, number] | null;
  canPlace(voxel: readonly [number, number, number], blockId: number): boolean;
  place(voxel: readonly [number, number, number], blockId: number): boolean;
}

export type PlaceAttemptResult =
  | { kind: "noop" }
  | {
      kind: "placed";
      voxel: [number, number, number];
      resource: LocalResourceKey;
      inventory: ReadonlyArray<LocalInventorySlot | null>;
      nextCooldownUntil: number;
    };

/**
 * 点按立刻放；按住等冷却。工具槽不放置。
 */
export function attemptPlace(input: PlaceAttemptInput): PlaceAttemptResult {
  if (!input.pointerLocked || !input.secondaryHeld) return { kind: "noop" };
  if (isToolHotbarSlot(input.selectedSlot)) return { kind: "noop" };

  const rising = !input.wasSecondaryHeld;
  const cooled = input.now >= input.placeCooldownUntil;
  if (!rising && !cooled) return { kind: "noop" };

  const potential = input.potential;
  if (potential === null) return { kind: "noop" };

  const slot = input.inventory[input.selectedSlot];
  if (slot === null || slot.quantity <= 0) return { kind: "noop" };
  const blockId = placeBlockIdForResource(slot.resource);
  if (blockId === null) return { kind: "noop" };
  if (!input.canPlace(potential, blockId)) return { kind: "noop" };
  if (!input.place(potential, blockId)) return { kind: "noop" };

  const consumed = consumeInventorySlot(input.inventory, input.selectedSlot);
  if (consumed.resource === null) return { kind: "noop" };

  return {
    kind: "placed",
    voxel: potential,
    resource: consumed.resource,
    inventory: consumed.inventory,
    nextCooldownUntil: input.now + LOCAL_PLACE_COOLDOWN_MS,
  };
}

/** 放置成功后清理耐久 claim 所需的 voxel key */
export function placeVoxelKey(
  voxel: readonly [number, number, number],
): string {
  return voxelKey(voxel);
}
