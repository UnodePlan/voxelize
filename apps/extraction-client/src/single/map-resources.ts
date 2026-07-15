import { LOCAL_BLOCK_IDS } from "./blocks";
import {
  LOCAL_EXTRACTION_XZ,
  LOCAL_RESOURCE_EXTRACT_CLEAR_RADIUS,
  LOCAL_RESOURCE_QUOTAS,
  LOCAL_RESOURCE_SPAWN_CLEAR_RADIUS,
  LOCAL_SPAWN_XZ,
} from "./map-layout";
import {
  LOCAL_BORDER_WALL_THICKNESS,
  LOCAL_WORLD_MAX,
  LOCAL_WORLD_MIN,
  type LocalBorderMode,
  type LocalQuarryMap,
} from "./map";
import type { LocalVoxel } from "./state";

type ChunkIndex = Map<string, LocalQuarryMap["chunks"][number]>;

type SetVoxelFn = (
  chunks: ChunkIndex,
  x: number,
  y: number,
  z: number,
  id: number,
) => void;

/** 运行时默认：非零 32-bit 随机种子 */
export function createMapSeed(random: () => number = Math.random): number {
  const value = Math.floor(random() * 0xffff_ffff) >>> 0;
  return value === 0 ? 1 : value;
}

/**
 * 按种子在地表散布泥土/黄金/钻石。
 * - 避开出生营与撤离广场
 * - 钻石偏好更远、更低处；黄金中等距离；泥土较近可刷
 * - 矿点上方清出 2 格空气，保证可挖
 */
export function placeSeededResources(
  chunks: ChunkIndex,
  heightAt: (x: number, z: number) => number,
  seed: number,
  setVoxel: SetVoxelFn,
  isInWorld: (x: number, z: number) => boolean,
  borderMode: LocalBorderMode = "bedrock",
): { voxels: LocalVoxel[]; ids: number[] } {
  const occupied = new Set<string>();
  const voxels: LocalVoxel[] = [];
  const ids: number[] = [];
  const rng = mulberry32(seed ^ 0x0e50_12ce);

  const plan: Array<{ id: number; count: number; minDist: number; preferLow: boolean }> =
    [
      {
        id: LOCAL_BLOCK_IDS.dirt,
        count: LOCAL_RESOURCE_QUOTAS.dirt,
        minDist: 4,
        preferLow: false,
      },
      {
        id: LOCAL_BLOCK_IDS.gold,
        count: LOCAL_RESOURCE_QUOTAS.gold,
        minDist: 9,
        preferLow: false,
      },
      {
        id: LOCAL_BLOCK_IDS.diamond,
        count: LOCAL_RESOURCE_QUOTAS.diamond,
        minDist: 14,
        preferLow: true,
      },
    ];

  // 基岩界避开墙带；虚空界只需留 1 格边距
  const margin =
    borderMode === "bedrock" ? LOCAL_BORDER_WALL_THICKNESS + 2 : 1;
  const xMin = LOCAL_WORLD_MIN + margin;
  const xMax = LOCAL_WORLD_MAX - margin;
  const zMin = LOCAL_WORLD_MIN + margin;
  const zMax = LOCAL_WORLD_MAX - margin;

  for (const entry of plan) {
    let placed = 0;
    let attempts = 0;
    const maxAttempts = entry.count * 80;
    while (placed < entry.count && attempts < maxAttempts) {
      attempts += 1;
      const x = randomInt(rng, xMin, xMax);
      const z = randomInt(rng, zMin, zMax);
      if (!isInWorld(x, z)) continue;
      if (isProtectedPlayArea(x, z)) continue;
      const key = `${x},${z}`;
      if (occupied.has(key)) continue;

      const distSpawn = Math.hypot(x - LOCAL_SPAWN_XZ[0], z - LOCAL_SPAWN_XZ[1]);
      if (distSpawn < entry.minDist) continue;

      // 钻石：拒绝过高台地，略偏爱低地
      const y = heightAt(x, z);
      if (entry.preferLow && y > 18 && rng() > 0.35) continue;

      occupied.add(key);
      setVoxel(chunks, x, y, z, entry.id);
      setVoxel(chunks, x, y + 1, z, LOCAL_BLOCK_IDS.air);
      setVoxel(chunks, x, y + 2, z, LOCAL_BLOCK_IDS.air);
      voxels.push([x, y, z]);
      ids.push(entry.id);
      placed += 1;
    }
  }

  return { voxels, ids };
}

function isProtectedPlayArea(x: number, z: number): boolean {
  const spawnDist = Math.hypot(x - LOCAL_SPAWN_XZ[0], z - LOCAL_SPAWN_XZ[1]);
  if (spawnDist < LOCAL_RESOURCE_SPAWN_CLEAR_RADIUS) return true;
  const extractDist = Math.hypot(
    x - LOCAL_EXTRACTION_XZ[0],
    z - LOCAL_EXTRACTION_XZ[1],
  );
  return extractDist < LOCAL_RESOURCE_EXTRACT_CLEAR_RADIUS;
}

function mulberry32(seed: number): () => number {
  let t = seed >>> 0;
  return () => {
    t += 0x6d_2b_79_f5;
    let r = Math.imul(t ^ (t >>> 15), 1 | t);
    r ^= r + Math.imul(r ^ (r >>> 7), 61 | r);
    return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
  };
}

function randomInt(rng: () => number, min: number, max: number): number {
  return min + Math.floor(rng() * (max - min + 1));
}
