import { describe, expect, it } from "vitest";

import { LOCAL_BLOCK_IDS, LOCAL_MINEABLE_BLOCKS } from "./blocks";
import {
  formatMapStyleLabel,
  LOCAL_BASE_HEIGHT,
  LOCAL_CHUNK_COUNT,
  LOCAL_CHUNK_SIZE,
  LOCAL_HEIGHT_AMP,
  LOCAL_MAP_SIZE,
  LOCAL_MAX_HEIGHT,
  LOCAL_TERRAIN_SEED,
  LOCAL_WORLD_MAX,
  LOCAL_WORLD_MIN,
  createLocalQuarryMap,
  getLocalMapVoxel,
  localMapChecksum,
  pickBorderMode,
} from "./map";
import { LOCAL_RESOURCE_QUOTAS } from "./map-layout";
import { LOCAL_MAP_STYLES, pickMapStyle } from "./map-style";
import { isBlockMineable } from "./blocks";

describe("local random grassland map", () => {
  it("is deterministic for the same seed and varies across seeds", () => {
    const a = createLocalQuarryMap(LOCAL_TERRAIN_SEED);
    const b = createLocalQuarryMap(LOCAL_TERRAIN_SEED);
    const c = createLocalQuarryMap(LOCAL_TERRAIN_SEED ^ 0xdead_beef);

    expect(LOCAL_MAP_SIZE).toBe(128);
    expect(LOCAL_WORLD_MAX - LOCAL_WORLD_MIN + 1).toBe(128);
    expect(a.chunks).toHaveLength(LOCAL_CHUNK_COUNT);
    expect(a.seed).toBe(LOCAL_TERRAIN_SEED);
    expect(a.borderMode).toBe(pickBorderMode(LOCAL_TERRAIN_SEED));
    expect(localMapChecksum(a)).toBe(localMapChecksum(b));
    expect(localMapChecksum(a)).not.toBe(localMapChecksum(c));
    expect(
      a.chunks.every(
        (chunk) =>
          chunk.voxels.length ===
          LOCAL_CHUNK_SIZE * LOCAL_MAX_HEIGHT * LOCAL_CHUNK_SIZE,
      ),
    ).toBe(true);
    expect(formatMapStyleLabel(a)).toContain(a.style.label);
    expect(formatMapStyleLabel(a)).toMatch(/基岩界|虚空界/);
  });

  it("bedrock border: walls and floor are unmineable bedrock", () => {
    const map = createLocalQuarryMap(LOCAL_TERRAIN_SEED, "bedrock");
    expect(map.borderMode).toBe("bedrock");
    // 四周墙
    expect(getLocalMapVoxel(map, LOCAL_WORLD_MIN, 12, 0)).toBe(
      LOCAL_BLOCK_IDS.bedrock,
    );
    expect(getLocalMapVoxel(map, LOCAL_WORLD_MAX, 12, 0)).toBe(
      LOCAL_BLOCK_IDS.bedrock,
    );
    expect(getLocalMapVoxel(map, 0, 12, LOCAL_WORLD_MIN)).toBe(
      LOCAL_BLOCK_IDS.bedrock,
    );
    // 底部基岩不可挖
    expect(getLocalMapVoxel(map, 10, 0, 10)).toBe(LOCAL_BLOCK_IDS.bedrock);
    expect(isBlockMineable(LOCAL_BLOCK_IDS.bedrock)).toBe(false);
    // 墙外未生成
    expect(getLocalMapVoxel(map, LOCAL_WORLD_MIN - 1, 12, 0)).toBe(
      LOCAL_BLOCK_IDS.air,
    );
  });

  it("void border: no walls, diggable floor, open sky at edges", () => {
    const map = createLocalQuarryMap(LOCAL_TERRAIN_SEED, "void");
    expect(map.borderMode).toBe("void");
    // 边界格是地形（非整柱基岩墙）
    const edgeTop = map.surfaceY(LOCAL_WORLD_MIN, 0);
    const edgeId = getLocalMapVoxel(map, LOCAL_WORLD_MIN, edgeTop, 0);
    expect(edgeId).not.toBe(LOCAL_BLOCK_IDS.air);
    // 边上方是空气（露虚空）
    expect(getLocalMapVoxel(map, LOCAL_WORLD_MIN, edgeTop + 2, 0)).toBe(
      LOCAL_BLOCK_IDS.air,
    );
    // 底部可挖穿（普通岩石，非基岩）
    const floorId = getLocalMapVoxel(map, 10, 0, 10);
    expect(floorId).not.toBe(LOCAL_BLOCK_IDS.bedrock);
    expect(isBlockMineable(floorId)).toBe(true);
    // 墙外仍是空气
    expect(getLocalMapVoxel(map, LOCAL_WORLD_MIN - 1, 12, 0)).toBe(
      LOCAL_BLOCK_IDS.air,
    );
  });

  it("has designed height variation and solid surface tops", () => {
    const map = createLocalQuarryMap(LOCAL_TERRAIN_SEED);
    const heights = new Set<number>();
    // 采样内圈，避开边界墙
    const margin = 4;
    for (let x = LOCAL_WORLD_MIN + margin; x <= LOCAL_WORLD_MAX - margin; x += 4) {
      for (
        let z = LOCAL_WORLD_MIN + margin;
        z <= LOCAL_WORLD_MAX - margin;
        z += 4
      ) {
        heights.add(map.surfaceY(x, z));
      }
    }
    expect(heights.size).toBeGreaterThan(4);
    for (const h of heights) {
      expect(h).toBeGreaterThanOrEqual(5);
      expect(h).toBeLessThan(LOCAL_MAX_HEIGHT);
      // 各 biome 幅度不同，放宽上界
      expect(h).toBeLessThanOrEqual(LOCAL_BASE_HEIGHT + LOCAL_HEIGHT_AMP + 12);
    }

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

  it("keeps continuous solid ground near spawn (no instant void island)", () => {
    const map = createLocalQuarryMap(LOCAL_TERRAIN_SEED);
    const [sx, , sz] = map.spawnFloor;
    // 出生点周围 24 格采样：地表必须是实体，避免开局大片空气
    let solid = 0;
    let total = 0;
    for (let dx = -24; dx <= 24; dx += 2) {
      for (let dz = -24; dz <= 24; dz += 2) {
        const x = sx + dx;
        const z = sz + dz;
        if (
          x <= LOCAL_WORLD_MIN + 2 ||
          x >= LOCAL_WORLD_MAX - 2 ||
          z <= LOCAL_WORLD_MIN + 2 ||
          z >= LOCAL_WORLD_MAX - 2
        ) {
          continue;
        }
        total += 1;
        const y = map.surfaceY(x, z);
        const id = getLocalMapVoxel(map, x, y, z);
        if (id !== LOCAL_BLOCK_IDS.air) solid += 1;
      }
    }
    expect(total).toBeGreaterThan(100);
    expect(solid / total).toBeGreaterThan(0.98);
    // 地图足够大：出生点到最近边界 > 40（60 地图时约 20，会一眼看到虚空）
    const toEdge = Math.min(
      sx - LOCAL_WORLD_MIN,
      LOCAL_WORLD_MAX - sx,
      sz - LOCAL_WORLD_MIN,
      LOCAL_WORLD_MAX - sz,
    );
    expect(toEdge).toBeGreaterThan(40);
  });

  it("supports spawn headroom and seeded resource quotas", () => {
    const map = createLocalQuarryMap(LOCAL_TERRAIN_SEED);
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
    expect(map.resourceCounts).toEqual({
      dirt: LOCAL_RESOURCE_QUOTAS.dirt,
      gold: LOCAL_RESOURCE_QUOTAS.gold,
      diamond: LOCAL_RESOURCE_QUOTAS.diamond,
    });

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

  it("scatters ore differently when the map seed changes", () => {
    const a = createLocalQuarryMap(0x1111_aaaa);
    const b = createLocalQuarryMap(0x2222_bbbb);
    const keysA = a.resourceVoxels.map((v) => v.join(",")).sort();
    const keysB = b.resourceVoxels.map((v) => v.join(",")).sort();
    expect(keysA).not.toEqual(keysB);
    expect(a.resourceCounts.gold).toBe(LOCAL_RESOURCE_QUOTAS.gold);
    expect(b.resourceCounts.diamond).toBe(LOCAL_RESOURCE_QUOTAS.diamond);
  });

  it("varies terrain shape across biomes and across seeds within a biome", () => {
    // 固定不同风格的种子（style = seed % styles.length）
    const desert = createLocalQuarryMap(2); // desert_day 若 index 2
    const snow = createLocalQuarryMap(4);
    const waste = createLocalQuarryMap(8);
    // 若长度变化，只要求「至少两种不同高度剖面」
    const profiles = [desert, snow, waste].map((m) => {
      const sample: number[] = [];
      for (let x = LOCAL_WORLD_MIN; x <= LOCAL_WORLD_MAX; x += 6) {
        sample.push(m.surfaceY(x, 0));
      }
      return sample.join(",");
    });
    expect(new Set(profiles).size).toBeGreaterThan(1);

    // 同风格 id、不同 seed：高度场应不同（局内变化）
    const s0 = LOCAL_MAP_STYLES[0];
    let seedA = 0;
    let seedB = 0;
    for (let s = 0; s < 200; s += 1) {
      if (pickMapStyle(s).id === s0.id) {
        if (seedA === 0) seedA = s;
        else if (s !== seedA) {
          seedB = s;
          break;
        }
      }
    }
    if (seedB !== 0) {
      const a = createLocalQuarryMap(seedA);
      const b = createLocalQuarryMap(seedB);
      expect(a.style.id).toBe(b.style.id);
      expect(localMapChecksum(a)).not.toBe(localMapChecksum(b));
    }
  });

  it("picks a stable map style from the seed among curated biome/time sets", () => {
    expect(LOCAL_MAP_STYLES.length).toBeGreaterThanOrEqual(12);
    const styleA = pickMapStyle(LOCAL_TERRAIN_SEED);
    const styleB = pickMapStyle(LOCAL_TERRAIN_SEED);
    expect(styleA.id).toBe(styleB.id);
    const map = createLocalQuarryMap(LOCAL_TERRAIN_SEED);
    expect(map.style.id).toBe(styleA.id);
    expect(map.style.label.length).toBeGreaterThan(2);
    expect(map.style.ambientColor.length).toBeGreaterThan(0);
    const ids = new Set(
      Array.from({ length: LOCAL_MAP_STYLES.length }, (_, n) =>
        pickMapStyle(n).id,
      ),
    );
    expect(ids.size).toBe(LOCAL_MAP_STYLES.length);
    // 覆盖雨林、春天与血月
    expect(LOCAL_MAP_STYLES.some((s) => s.biome === "rainforest")).toBe(true);
    expect(LOCAL_MAP_STYLES.some((s) => s.biome === "spring")).toBe(true);
    expect(LOCAL_MAP_STYLES.some((s) => s.dayPhase === "blood")).toBe(true);
  });

  it("spring biome places grass and leafy trees without water", () => {
    // 找一个 spring 风格种子
    let springSeed = -1;
    for (let s = 0; s < LOCAL_MAP_STYLES.length * 4; s += 1) {
      if (pickMapStyle(s).biome === "spring") {
        springSeed = s;
        break;
      }
    }
    expect(springSeed).toBeGreaterThanOrEqual(0);
    const map = createLocalQuarryMap(springSeed);
    expect(map.style.biome).toBe("spring");

    let grass = 0;
    let leaves = 0;
    let timber = 0;
    for (let x = LOCAL_WORLD_MIN; x <= LOCAL_WORLD_MAX; x += 2) {
      for (let z = LOCAL_WORLD_MIN; z <= LOCAL_WORLD_MAX; z += 2) {
        const top = map.surfaceY(x, z);
        for (let y = top; y <= top + 8 && y < LOCAL_MAX_HEIGHT; y += 1) {
          const id = getLocalMapVoxel(map, x, y, z);
          if (id === LOCAL_BLOCK_IDS.grass) grass += 1;
          if (id === LOCAL_BLOCK_IDS.leaves) leaves += 1;
          if (id === LOCAL_BLOCK_IDS.weatheredTimber) timber += 1;
        }
      }
    }
    expect(grass).toBeGreaterThan(20);
    expect(leaves).toBeGreaterThan(5);
    expect(timber).toBeGreaterThan(2);
    // 项目明确不生成水
    expect("water" in LOCAL_BLOCK_IDS).toBe(false);
  });

  it("spawns outside the extraction radius so extraction is always deliberate", () => {
    const map = createLocalQuarryMap(LOCAL_TERRAIN_SEED);
    const [spawnX, , spawnZ] = map.spawn;
    const [zoneX, , zoneZ] = map.extraction.center;

    expect(Math.hypot(spawnX - zoneX, spawnZ - zoneZ)).toBeGreaterThan(
      map.extraction.radius,
    );
  });

  it("places the opening spawn high above the camp floor for a sky drop-in", () => {
    const map = createLocalQuarryMap(LOCAL_TERRAIN_SEED);
    const [, floorY] = map.spawnFloor;
    const [, eyeY] = map.spawn;
    expect(eyeY).toBeGreaterThan(floorY + 20);
    expect(eyeY).toBeLessThan(LOCAL_MAX_HEIGHT);
  });

  it("keeps the authored return route continuous without mining", () => {
    const map = createLocalQuarryMap(LOCAL_TERRAIN_SEED);

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
