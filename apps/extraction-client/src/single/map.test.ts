import { describe, expect, it } from "vitest";

import { LOCAL_BLOCK_IDS, LOCAL_MINEABLE_BLOCKS } from "./blocks";
import {
  LOCAL_BASE_HEIGHT,
  LOCAL_CHUNK_COUNT,
  LOCAL_CHUNK_SIZE,
  LOCAL_HEIGHT_AMP,
  LOCAL_MAP_SIZE,
  LOCAL_MAX_HEIGHT,
  LOCAL_WORLD_MAX,
  LOCAL_WORLD_MIN,
  createLocalQuarryMap,
  getLocalMapVoxel,
  localMapChecksum,
} from "./map";

describe("local random grassland map", () => {
  it("generates a deterministic 60x60 world with open sky edges", () => {
    const first = createLocalQuarryMap();
    const second = createLocalQuarryMap();

    expect(LOCAL_MAP_SIZE).toBe(60);
    expect(LOCAL_WORLD_MAX - LOCAL_WORLD_MIN + 1).toBe(60);
    expect(first.chunks).toHaveLength(LOCAL_CHUNK_COUNT);
    expect(localMapChecksum(first)).toBe(localMapChecksum(second));
    expect(
      first.chunks.every(
        (chunk) =>
          chunk.voxels.length ===
          LOCAL_CHUNK_SIZE * LOCAL_MAX_HEIGHT * LOCAL_CHUNK_SIZE,
      ),
    ).toBe(true);

    // 边界无墙：边上方是空气（旁边露天空）
    const edgeX = LOCAL_WORLD_MIN;
    const edgeTop = first.surfaceY(edgeX, 0);
    expect(getLocalMapVoxel(first, edgeX, edgeTop + 1, 0)).toBe(
      LOCAL_BLOCK_IDS.air,
    );
    // 世界外直接是空气
    expect(getLocalMapVoxel(first, LOCAL_WORLD_MIN - 1, edgeTop, 0)).toBe(
      LOCAL_BLOCK_IDS.air,
    );
    expect(getLocalMapVoxel(first, LOCAL_WORLD_MAX + 1, edgeTop, 0)).toBe(
      LOCAL_BLOCK_IDS.air,
    );
  });

  it("has designed height variation and solid surface tops", () => {
    const map = createLocalQuarryMap();
    const heights = new Set<number>();
    for (let x = LOCAL_WORLD_MIN; x <= LOCAL_WORLD_MAX; x += 4) {
      for (let z = LOCAL_WORLD_MIN; z <= LOCAL_WORLD_MAX; z += 4) {
        heights.add(map.surfaceY(x, z));
      }
    }
    // 应有起伏，不是整张平地
    expect(heights.size).toBeGreaterThan(4);
    for (const h of heights) {
      expect(h).toBeGreaterThanOrEqual(5);
      expect(h).toBeLessThan(LOCAL_MAX_HEIGHT);
      expect(h).toBeLessThanOrEqual(LOCAL_BASE_HEIGHT + LOCAL_HEIGHT_AMP + 2);
    }

    // 抽样：地表为实心块（草/土/石/玩法覆盖），上方空气
    const sampleX = 20;
    const sampleZ = 20;
    const top = map.surfaceY(sampleX, sampleZ);
    const topId = getLocalMapVoxel(map, sampleX, top, sampleZ);
    expect(topId).not.toBe(LOCAL_BLOCK_IDS.air);
    expect(topId).not.toBe(LOCAL_BLOCK_IDS.bedrock);
    expect(getLocalMapVoxel(map, sampleX, top + 1, sampleZ)).toBe(
      LOCAL_BLOCK_IDS.air,
    );

    // 撤离广场应有石板
    expect(getLocalMapVoxel(map, 0, map.surfaceY(0, 0), 0)).toBe(
      LOCAL_BLOCK_IDS.paleStone,
    );
  });

  it("supports spawn headroom and exposes all resource tiers", () => {
    const map = createLocalQuarryMap();
    const [x, y, z] = map.spawnFloor;
    const neighbors = [
      [1, 0, 0],
      [-1, 0, 0],
      [0, 1, 0],
      [0, -1, 0],
      [0, 0, 1],
      [0, 0, -1],
    ] as const;

    expect(getLocalMapVoxel(map, x, y, z)).not.toBe(LOCAL_BLOCK_IDS.air);
    expect(getLocalMapVoxel(map, x, y + 1, z)).toBe(LOCAL_BLOCK_IDS.air);
    expect(map.resourceCounts).toEqual({ dirt: 5, gold: 5, diamond: 5 });

    const remaining = new Map(
      map.resourceVoxels.map((voxel) => [voxel.join(","), voxel]),
    );
    for (const [rx, ry, rz] of map.resourceVoxels) {
      expect(
        LOCAL_MINEABLE_BLOCKS[getLocalMapVoxel(map, rx, ry, rz)],
      ).toBeDefined();
    }
    let removedInPass = true;
    const removed = new Set<string>();
    while (remaining.size > 0 && removedInPass) {
      removedInPass = false;
      for (const [key, [rx, ry, rz]] of remaining) {
        const accessible = neighbors.some(([dx, dy, dz]) => {
          const neighborKey = [rx + dx, ry + dy, rz + dz].join(",");
          return (
            removed.has(neighborKey) ||
            getLocalMapVoxel(map, rx + dx, ry + dy, rz + dz) ===
              LOCAL_BLOCK_IDS.air
          );
        });
        if (!accessible) continue;
        remaining.delete(key);
        removed.add(key);
        removedInPass = true;
      }
    }
    expect([...remaining.keys()]).toEqual([]);
  });

  it("spawns outside the extraction radius so extraction is always deliberate", () => {
    const map = createLocalQuarryMap();
    const [spawnX, , spawnZ] = map.spawn;
    const [zoneX, , zoneZ] = map.extraction.center;

    expect(Math.hypot(spawnX - zoneX, spawnZ - zoneZ)).toBeGreaterThan(
      map.extraction.radius,
    );
  });

  it("keeps the authored return route continuous without mining", () => {
    const map = createLocalQuarryMap();

    for (let index = 0; index < map.route.length; index += 1) {
      const [x, y, z] = map.route[index];
      expect(getLocalMapVoxel(map, x, y, z)).not.toBe(LOCAL_BLOCK_IDS.air);
      expect(getLocalMapVoxel(map, x, y + 1, z)).toBe(LOCAL_BLOCK_IDS.air);
      if (index === 0) continue;
      const previous = map.route[index - 1];
      expect(Math.abs(y - previous[1])).toBeLessThanOrEqual(1);
      expect(Math.abs(x - previous[0]) + Math.abs(z - previous[2])).toBe(1);
    }
  });
});
