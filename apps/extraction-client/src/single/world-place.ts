/**
 * 本地世界方块放置判定与写入。
 * 对齐 examples 右键放置：空邻格、高度、玩家 AABB。
 */

import type { RigidControls, World } from "@voxelize/core";

import type { LocalWorldAdapter } from "./world-adapter";

export interface PlaceWorldContext {
  ready: boolean;
  disposed: boolean;
  world: World;
  controls: RigidControls;
  adapter: LocalWorldAdapter;
}

/**
 * 是否可在 potential 邻格放置。
 * 不可穿越块不得与玩家身体相交。
 */
export function canPlaceLocalBlock(
  ctx: PlaceWorldContext,
  voxel: readonly [number, number, number],
  blockId: number,
): boolean {
  if (!ctx.ready || ctx.disposed) return false;
  const [vx, vy, vz] = voxel;
  if (vy < 0 || vy >= ctx.world.options.maxHeight) return false;
  if (ctx.world.getVoxelAt(vx, vy, vz) !== 0) return false;
  const block = ctx.world.getBlockById(blockId);
  if (block === null || block === undefined) return false;
  if (!block.isPassable) {
    try {
      const aabbs = ctx.world.getBlockAABBsByIdAt(blockId, vx, vy, vz);
      const body = ctx.controls.body.aabb;
      for (const aabb of aabbs) {
        if (aabb.clone().translate([vx, vy, vz]).intersects(body)) {
          return false;
        }
      }
    } catch {
      // 无 AABB 定义时仍允许放置
    }
  }
  return true;
}

/**
 * 写入本地体素（server 源立即生效）。
 * @returns 是否写入成功
 */
export function placeLocalBlock(
  ctx: PlaceWorldContext,
  voxel: readonly [number, number, number],
  blockId: number,
): boolean {
  if (!canPlaceLocalBlock(ctx, voxel, blockId)) return false;
  try {
    ctx.adapter.applyServerVoxelUpdate(ctx.world, voxel, blockId);
    return true;
  } catch {
    return false;
  }
}
