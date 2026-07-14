import type { ChunkProtocol } from "@voxelize/protocol";

import type { ResourceCounts } from "../api/models";

import { LOCAL_BLOCK_IDS, resourceCountsFromIds } from "./blocks";
import {
  LOCAL_EXTRACTION_XZ,
  LOCAL_RESOURCE_SPOTS,
  LOCAL_ROUTE_XZ,
  LOCAL_SPAWN_XZ,
} from "./map-layout";
import type { LocalVoxel } from "./state";

export const LOCAL_CHUNK_SIZE = 16;
export const LOCAL_MAX_HEIGHT = 48;
export const LOCAL_SUB_CHUNKS = 3;
/** 可玩平面边长（方块）；边界外无墙，直接露天空 */
export const LOCAL_MAP_SIZE = 60;
/** 世界 x/z 闭区间，以 0 为中心：[-30, 29] */
export const LOCAL_WORLD_MIN = -Math.floor(LOCAL_MAP_SIZE / 2);
export const LOCAL_WORLD_MAX = LOCAL_WORLD_MIN + LOCAL_MAP_SIZE - 1;
export const LOCAL_MIN_CHUNK = [
  Math.floor(LOCAL_WORLD_MIN / LOCAL_CHUNK_SIZE),
  Math.floor(LOCAL_WORLD_MIN / LOCAL_CHUNK_SIZE),
] as const;
export const LOCAL_MAX_CHUNK = [
  Math.floor(LOCAL_WORLD_MAX / LOCAL_CHUNK_SIZE),
  Math.floor(LOCAL_WORLD_MAX / LOCAL_CHUNK_SIZE),
] as const;
export const LOCAL_CHUNK_COUNT =
  (LOCAL_MAX_CHUNK[0] - LOCAL_MIN_CHUNK[0] + 1) *
  (LOCAL_MAX_CHUNK[1] - LOCAL_MIN_CHUNK[1] + 1);

/** 固定种子：可复现的“设计感”随机地形 */
export const LOCAL_TERRAIN_SEED = 0x51_4e_47_4c;
/** 高度图中心基准 */
export const LOCAL_BASE_HEIGHT = 16;
/** 起伏幅度（含边缘落差，大致地表 ∈ [BASE-AMP, BASE+AMP]） */
export const LOCAL_HEIGHT_AMP = 12;

/** @deprecated 兼容旧引用 */
export const LOCAL_SURFACE_Y = LOCAL_BASE_HEIGHT;

const SPAWN_EYE_OFFSET = 1.62;
const DIRT_DEPTH = 3;

export interface LocalExtractionZone {
  center: readonly [number, number, number];
  radius: number;
}

export interface LocalQuarryMap {
  chunks: ChunkProtocol[];
  extraction: LocalExtractionZone;
  initialDirection: readonly [number, number, number];
  resourceCounts: ResourceCounts;
  resourceVoxels: ReadonlyArray<LocalVoxel>;
  route: ReadonlyArray<LocalVoxel>;
  spawn: readonly [number, number, number];
  spawnFloor: LocalVoxel;
  /** 查询地表高度（与生成一致） */
  surfaceY: (x: number, z: number) => number;
}

export function createLocalQuarryMap(): LocalQuarryMap {
  const heightAt = createHeightFunction(LOCAL_TERRAIN_SEED);
  const chunks = createEmptyChunks();
  const chunkIndex = indexChunks(chunks);

  fillDesignedTerrain(chunks, heightAt, LOCAL_TERRAIN_SEED);
  decorateTerrain(chunkIndex, heightAt, LOCAL_TERRAIN_SEED);

  // 撤离广场 / 出生小营：玩法可读性优先
  stampExtractionPlaza(chunkIndex, heightAt);
  stampSpawnCamp(chunkIndex, heightAt);

  const route = createSurfaceRoute(LOCAL_ROUTE_XZ, heightAt);
  stampRoute(chunkIndex, route, heightAt);

  const resourceVoxels: LocalVoxel[] = [];
  const resourceIds: number[] = [];
  for (const [x, z, id] of LOCAL_RESOURCE_SPOTS) {
    if (!isInWorld(x, z)) continue;
    const y = heightAt(x, z);
    setVoxel(chunkIndex, x, y, z, id);
    // 保证矿点头顶可站/可挖
    setVoxel(chunkIndex, x, y + 1, z, LOCAL_BLOCK_IDS.air);
    setVoxel(chunkIndex, x, y + 2, z, LOCAL_BLOCK_IDS.air);
    resourceVoxels.push([x, y, z]);
    resourceIds.push(id);
  }

  fillInitialLight(chunks);

  const [sx, sz] = LOCAL_SPAWN_XZ;
  const spawnY = heightAt(sx, sz);
  const spawnFloor: LocalVoxel = [sx, spawnY, sz];
  const [ex, ez] = LOCAL_EXTRACTION_XZ;
  const extractY = heightAt(ex, ez);

  return {
    chunks,
    spawn: [sx + 0.5, spawnY + SPAWN_EYE_OFFSET, sz + 0.5],
    spawnFloor,
    initialDirection: [0, 0, -1],
    extraction: {
      center: [ex + 0.5, extractY + 0.05, ez + 0.5],
      radius: 2.1,
    },
    route,
    resourceVoxels,
    resourceCounts: resourceCountsFromIds(resourceIds),
    surfaceY: heightAt,
  };
}

export function getLocalMapVoxel(
  map: Pick<LocalQuarryMap, "chunks">,
  x: number,
  y: number,
  z: number,
): number {
  if (y < 0 || y >= LOCAL_MAX_HEIGHT) return LOCAL_BLOCK_IDS.air;
  const cx = Math.floor(x / LOCAL_CHUNK_SIZE);
  const cz = Math.floor(z / LOCAL_CHUNK_SIZE);
  const chunk = map.chunks.find(
    (candidate) => candidate.x === cx && candidate.z === cz,
  );
  if (chunk === undefined) return LOCAL_BLOCK_IDS.air;
  const lx = x - cx * LOCAL_CHUNK_SIZE;
  const lz = z - cz * LOCAL_CHUNK_SIZE;
  return chunk.voxels[voxelIndex(lx, y, lz)] & 0xffff;
}

export function localMapChecksum(map: Pick<LocalQuarryMap, "chunks">): number {
  let hash = 2_166_136_261;
  for (const chunk of map.chunks) {
    for (const voxel of chunk.voxels) {
      hash ^= voxel;
      hash = Math.imul(hash, 16_777_619);
    }
  }
  return hash >>> 0;
}

// ——— 高度设计 ———
//
// 60×60「废弃采石草甸」：
// - 中心盆地：撤离区平坦
// - 出生台地：略抬、较平
// - 脊线丘陵 + 细节起伏
// - 边缘向天空跌落（无墙）

function createHeightFunction(seed: number): (x: number, z: number) => number {
  return (x, z) => {
    const hills = fbm(x * 0.035, z * 0.035, seed);
    const ridge = 1 - Math.abs(fbm(x * 0.05, z * 0.05, seed ^ 0xa11) * 2 - 1);
    const detail = fbm(x * 0.12, z * 0.12, seed ^ 0xb0b);
    // 东北高地、西南缓坡：给地图方向感
    const bias = (x + z) * 0.04;

    let h =
      LOCAL_BASE_HEIGHT +
      (hills - 0.5) * 9 +
      (ridge - 0.45) * 5 +
      (detail - 0.5) * 2.2 +
      bias;

    // 撤离盆地：压平并略降
    const extractDist = Math.hypot(x - LOCAL_EXTRACTION_XZ[0], z - LOCAL_EXTRACTION_XZ[1]);
    if (extractDist < 8) {
      const t = 1 - extractDist / 8;
      h = lerp(h, LOCAL_BASE_HEIGHT - 1, t * t * 0.9);
    }

    // 出生台地：略抬、平滑
    const spawnDist = Math.hypot(x - LOCAL_SPAWN_XZ[0], z - LOCAL_SPAWN_XZ[1]);
    if (spawnDist < 6) {
      const t = 1 - spawnDist / 6;
      h = lerp(h, LOCAL_BASE_HEIGHT + 2, t * t * 0.75);
    }

    // 地图边缘跌落，站在边上能看到天空断层
    const edge = edgeFalloff01(x, z);
    h -= edge * edge * 7;

    const y = Math.round(h);
    return Math.max(5, Math.min(LOCAL_MAX_HEIGHT - 6, y));
  };
}

/** 0=中心，1=贴边 */
function edgeFalloff01(x: number, z: number): number {
  const nx =
    (x - LOCAL_WORLD_MIN) / Math.max(1, LOCAL_WORLD_MAX - LOCAL_WORLD_MIN);
  const nz =
    (z - LOCAL_WORLD_MIN) / Math.max(1, LOCAL_WORLD_MAX - LOCAL_WORLD_MIN);
  const dx = Math.min(nx, 1 - nx) * 2; // 0 edge → 1 center
  const dz = Math.min(nz, 1 - nz) * 2;
  const centerish = Math.min(dx, dz);
  return Math.max(0, 1 - centerish / 0.35);
}

// ——— 体素填充与地表分区 ———

function fillDesignedTerrain(
  chunks: ChunkProtocol[],
  heightAt: (x: number, z: number) => number,
  seed: number,
): void {
  for (const chunk of chunks) {
    for (let lx = 0; lx < LOCAL_CHUNK_SIZE; lx += 1) {
      for (let lz = 0; lz < LOCAL_CHUNK_SIZE; lz += 1) {
        const x = chunk.x * LOCAL_CHUNK_SIZE + lx;
        const z = chunk.z * LOCAL_CHUNK_SIZE + lz;
        if (!isInWorld(x, z)) continue;

        const topY = heightAt(x, z);
        const topId = pickSurfaceBlock(x, z, topY, seed);
        for (let y = 0; y <= topY; y += 1) {
          let id: number;
          if (y === 0) {
            id = LOCAL_BLOCK_IDS.bedrock;
          } else if (y === topY) {
            id = topId;
          } else if (y >= topY - DIRT_DEPTH) {
            // 岩面下用石，草/土下用土
            id =
              topId === LOCAL_BLOCK_IDS.grass || topId === LOCAL_BLOCK_IDS.dirt
                ? LOCAL_BLOCK_IDS.dirt
                : LOCAL_BLOCK_IDS.quarryStone;
          } else if (y >= topY - 6 && hash01(x, y + z * 3, seed) > 0.82) {
            // 浅层夹杂化石带
            id = LOCAL_BLOCK_IDS.paleStone;
          } else {
            id = LOCAL_BLOCK_IDS.quarryStone;
          }
          setChunkVoxel(chunk, lx, y, lz, id);
        }
      }
    }
  }
}

/**
 * 地表材质分区：
 * - 低地：干土 / 浅色石（干河床感）
 * - 中地：草
 * - 高地 / 陡岩：风化石、采石岩
 */
function pickSurfaceBlock(
  x: number,
  z: number,
  topY: number,
  seed: number,
): number {
  const moisture = fbm(x * 0.07, z * 0.07, seed ^ 0xc0a);
  const rockiness = fbm(x * 0.11, z * 0.11, seed ^ 0xd0d);
  const creek = dryCreekMask(x, z, seed);

  if (creek > 0.62) return LOCAL_BLOCK_IDS.paleStone;
  if (topY <= LOCAL_BASE_HEIGHT - 3) {
    return moisture > 0.55 ? LOCAL_BLOCK_IDS.dirt : LOCAL_BLOCK_IDS.paleStone;
  }
  if (topY >= LOCAL_BASE_HEIGHT + 5 || rockiness > 0.74) {
    return rockiness > 0.82
      ? LOCAL_BLOCK_IDS.quarryStone
      : LOCAL_BLOCK_IDS.paleStone;
  }
  if (moisture < 0.3) return LOCAL_BLOCK_IDS.dirt;
  return LOCAL_BLOCK_IDS.grass;
}

/** 蜿蜒干河床 mask ∈ [0,1] */
function dryCreekMask(x: number, z: number, seed: number): number {
  // 沿对角线的正弦河谷 + 噪声扰动
  const meander =
    Math.sin((x * 0.35 + z * 0.12) + fbm(x * 0.05, z * 0.05, seed ^ 0xcec) * 4) *
    0.5 +
    0.5;
  const distToRibbon = Math.abs(
    z - (-4 + Math.sin(x * 0.22) * 6 + (fbm(x * 0.08, 0, seed) - 0.5) * 4),
  );
  const ribbon = Math.max(0, 1 - distToRibbon / 2.2);
  return Math.max(ribbon, meander * 0.15);
}

// ——— 装饰物 ———

function decorateTerrain(
  chunks: Map<string, ChunkProtocol>,
  heightAt: (x: number, z: number) => number,
  seed: number,
): void {
  // 高处岩柱 / 碎石堆
  for (let x = LOCAL_WORLD_MIN + 2; x <= LOCAL_WORLD_MAX - 2; x += 1) {
    for (let z = LOCAL_WORLD_MIN + 2; z <= LOCAL_WORLD_MAX - 2; z += 1) {
      const top = heightAt(x, z);
      const r = hash01(x, z, seed ^ 0xf11);
      // 稀疏岩柱
      if (top >= LOCAL_BASE_HEIGHT + 4 && r > 0.965) {
        const h = 2 + Math.floor(hash01(x + 3, z - 1, seed) * 3);
        for (let dy = 1; dy <= h; dy += 1) {
          setVoxel(
            chunks,
            x,
            top + dy,
            z,
            dy === h
              ? LOCAL_BLOCK_IDS.paleStone
              : LOCAL_BLOCK_IDS.quarryStone,
          );
        }
      }
      // 枯木桩（旧梁）
      if (r > 0.988 && r <= 0.995 && top >= LOCAL_BASE_HEIGHT - 1) {
        setVoxel(chunks, x, top + 1, z, LOCAL_BLOCK_IDS.weatheredTimber);
        if (hash01(z, x, seed) > 0.5) {
          setVoxel(chunks, x, top + 2, z, LOCAL_BLOCK_IDS.weatheredTimber);
        }
      }
    }
  }

  // 几处小废墟
  stampRuin(chunks, heightAt, 18, -12, seed);
  stampRuin(chunks, heightAt, -16, 14, seed ^ 1);
  stampRuin(chunks, heightAt, -20, -18, seed ^ 2);

  // 碎石圈（中型地标）
  stampRockRing(chunks, heightAt, 12, 16, 3);
  stampRockRing(chunks, heightAt, -14, -8, 2);
}

function stampRuin(
  chunks: Map<string, ChunkProtocol>,
  heightAt: (x: number, z: number) => number,
  cx: number,
  cz: number,
  seed: number,
): void {
  if (!isInWorld(cx, cz)) return;
  for (let dx = -2; dx <= 2; dx += 1) {
    for (let dz = -2; dz <= 2; dz += 1) {
      const x = cx + dx;
      const z = cz + dz;
      if (!isInWorld(x, z)) continue;
      const y = heightAt(x, z);
      // 地板
      if (Math.abs(dx) + Math.abs(dz) <= 3) {
        setColumnTop(chunks, x, z, y, LOCAL_BLOCK_IDS.paleStone, heightAt);
      }
    }
  }
  // 残柱
  for (const [dx, dz] of [
    [-2, -2],
    [2, -2],
    [-2, 2],
    [2, 2],
  ] as const) {
    const x = cx + dx;
    const z = cz + dz;
    if (!isInWorld(x, z)) continue;
    const y = heightAt(x, z);
    const h = 2 + Math.floor(hash01(x, z, seed) * 3);
    for (let dy = 1; dy <= h; dy += 1) {
      setVoxel(
        chunks,
        x,
        y + dy,
        z,
        dy === h
          ? LOCAL_BLOCK_IDS.weatheredTimber
          : LOCAL_BLOCK_IDS.paleStone,
      );
    }
  }
}

function stampRockRing(
  chunks: Map<string, ChunkProtocol>,
  heightAt: (x: number, z: number) => number,
  cx: number,
  cz: number,
  radius: number,
): void {
  for (let a = 0; a < Math.PI * 2; a += Math.PI / 6) {
    const x = Math.round(cx + Math.cos(a) * radius);
    const z = Math.round(cz + Math.sin(a) * radius);
    if (!isInWorld(x, z)) continue;
    const y = heightAt(x, z);
    setVoxel(chunks, x, y + 1, z, LOCAL_BLOCK_IDS.quarryStone);
    if ((x + z) % 2 === 0) {
      setVoxel(chunks, x, y + 2, z, LOCAL_BLOCK_IDS.paleStone);
    }
  }
}

function stampExtractionPlaza(
  chunks: Map<string, ChunkProtocol>,
  heightAt: (x: number, z: number) => number,
): void {
  const [ex, ez] = LOCAL_EXTRACTION_XZ;
  for (let dx = -3; dx <= 3; dx += 1) {
    for (let dz = -3; dz <= 3; dz += 1) {
      if (dx * dx + dz * dz > 10) continue;
      const x = ex + dx;
      const z = ez + dz;
      if (!isInWorld(x, z)) continue;
      const y = heightAt(x, z);
      setColumnTop(chunks, x, z, y, LOCAL_BLOCK_IDS.paleStone, heightAt);
    }
  }
  // 四角短柱当信标底座装饰
  for (const [dx, dz] of [
    [-3, 0],
    [3, 0],
    [0, -3],
    [0, 3],
  ] as const) {
    const x = ex + dx;
    const z = ez + dz;
    if (!isInWorld(x, z)) continue;
    const y = heightAt(x, z);
    setVoxel(chunks, x, y + 1, z, LOCAL_BLOCK_IDS.weatheredTimber);
  }
}

function stampSpawnCamp(
  chunks: Map<string, ChunkProtocol>,
  heightAt: (x: number, z: number) => number,
): void {
  const [sx, sz] = LOCAL_SPAWN_XZ;
  for (let dx = -2; dx <= 2; dx += 1) {
    for (let dz = -1; dz <= 2; dz += 1) {
      const x = sx + dx;
      const z = sz + dz;
      if (!isInWorld(x, z)) continue;
      const y = heightAt(x, z);
      setColumnTop(chunks, x, z, y, LOCAL_BLOCK_IDS.paleStone, heightAt);
    }
  }
  // 简易门廊
  for (const x of [sx - 2, sx + 2]) {
    const y = heightAt(x, sz + 2);
    for (let dy = 1; dy <= 3; dy += 1) {
      setVoxel(chunks, x, y + dy, sz + 2, LOCAL_BLOCK_IDS.paleStone);
    }
  }
  const beamY = heightAt(sx, sz + 2) + 3;
  for (let x = sx - 2; x <= sx + 2; x += 1) {
    setVoxel(chunks, x, beamY, sz + 2, LOCAL_BLOCK_IDS.weatheredTimber);
  }
}

/** 把 (x,z) 列顶压成指定块，并清空头顶（含装饰岩柱高度） */
function setColumnTop(
  chunks: Map<string, ChunkProtocol>,
  x: number,
  z: number,
  y: number,
  id: number,
  heightAt: (x: number, z: number) => number,
): void {
  const natural = heightAt(x, z);
  // 装饰岩柱最高约 +5，多清几格保证路线可走
  const clearTo = Math.max(natural, y) + 8;
  for (let yy = 1; yy < y; yy += 1) {
    setVoxel(
      chunks,
      x,
      yy,
      z,
      yy >= y - DIRT_DEPTH ? LOCAL_BLOCK_IDS.dirt : LOCAL_BLOCK_IDS.quarryStone,
    );
  }
  setVoxel(chunks, x, y, z, id);
  for (let yy = y + 1; yy <= clearTo; yy += 1) {
    setVoxel(chunks, x, yy, z, LOCAL_BLOCK_IDS.air);
  }
}

// ——— 路线 ———

function createSurfaceRoute(
  waypoints: ReadonlyArray<readonly [number, number]>,
  heightAt: (x: number, z: number) => number,
): LocalVoxel[] {
  const xzPath: Array<[number, number]> = [];
  for (let index = 0; index < waypoints.length - 1; index += 1) {
    const [x0, z0] = waypoints[index];
    const [x1, z1] = waypoints[index + 1];
    const distance = Math.abs(x1 - x0) + Math.abs(z1 - z0);
    for (let step = index === 0 ? 0 : 1; step <= distance; step += 1) {
      const progress = distance === 0 ? 0 : step / distance;
      xzPath.push([
        Math.round(x0 + (x1 - x0) * progress),
        Math.round(z0 + (z1 - z0) * progress),
      ]);
    }
  }

  const route: LocalVoxel[] = [];
  let prevY =
    xzPath.length > 0 ? heightAt(xzPath[0][0], xzPath[0][1]) : LOCAL_BASE_HEIGHT;
  for (const [x, z] of xzPath) {
    const target = heightAt(x, z);
    const y =
      target > prevY + 1 ? prevY + 1 : target < prevY - 1 ? prevY - 1 : target;
    route.push([x, y, z]);
    prevY = y;
  }
  return route;
}

function stampRoute(
  chunks: Map<string, ChunkProtocol>,
  route: ReadonlyArray<LocalVoxel>,
  heightAt: (x: number, z: number) => number,
): void {
  // 先铺主路，再拓宽；拓宽不得改写主路格，避免高低邻格互相盖住
  const routeKeys = new Set(route.map(([x, , z]) => `${x},${z}`));
  for (const [x, pathY, z] of route) {
    setColumnTop(chunks, x, z, pathY, LOCAL_BLOCK_IDS.paleStone, heightAt);
  }
  for (const [x, pathY, z] of route) {
    for (const [dx, dz] of [
      [1, 0],
      [-1, 0],
      [0, 1],
      [0, -1],
    ] as const) {
      const nx = x + dx;
      const nz = z + dz;
      if (!isInWorld(nx, nz) || routeKeys.has(`${nx},${nz}`)) continue;
      const ny = heightAt(nx, nz);
      // 邻格用自身地表高度铺石，且仅缓坡可拓宽
      if (Math.abs(ny - pathY) <= 1) {
        setColumnTop(chunks, nx, nz, ny, LOCAL_BLOCK_IDS.paleStone, heightAt);
      }
    }
  }
}

// ——— 噪声工具 ———

function fbm(x: number, z: number, seed: number): number {
  let amp = 1;
  let freq = 1;
  let sum = 0;
  let norm = 0;
  for (let octave = 0; octave < 4; octave += 1) {
    sum += amp * valueNoise(x * freq, z * freq, seed + octave * 1013);
    norm += amp;
    amp *= 0.5;
    freq *= 2;
  }
  return sum / norm;
}

function valueNoise(x: number, z: number, seed: number): number {
  const x0 = Math.floor(x);
  const z0 = Math.floor(z);
  const fx = smoothstep(x - x0);
  const fz = smoothstep(z - z0);
  const v00 = hash01(x0, z0, seed);
  const v10 = hash01(x0 + 1, z0, seed);
  const v01 = hash01(x0, z0 + 1, seed);
  const v11 = hash01(x0 + 1, z0 + 1, seed);
  return lerp(lerp(v00, v10, fx), lerp(v01, v11, fx), fz);
}

function hash01(x: number, z: number, seed: number): number {
  let h = Math.imul(x | 0, 374_761_393) ^ Math.imul(z | 0, 668_265_263) ^ seed;
  h = Math.imul(h ^ (h >>> 13), 1_274_126_177);
  h = h ^ (h >>> 16);
  return (h >>> 0) / 4_294_967_295;
}

function smoothstep(t: number): number {
  return t * t * (3 - 2 * t);
}

function lerp(a: number, b: number, t: number): number {
  return a + (b - a) * t;
}

// ——— 底层 chunk 工具 ———

function createEmptyChunks(): ChunkProtocol[] {
  const chunks: ChunkProtocol[] = [];
  const length = LOCAL_CHUNK_SIZE * LOCAL_MAX_HEIGHT * LOCAL_CHUNK_SIZE;
  for (let cx = LOCAL_MIN_CHUNK[0]; cx <= LOCAL_MAX_CHUNK[0]; cx += 1) {
    for (let cz = LOCAL_MIN_CHUNK[1]; cz <= LOCAL_MAX_CHUNK[1]; cz += 1) {
      chunks.push({
        id: `single:${cx}:${cz}`,
        x: cx,
        z: cz,
        meshes: [],
        voxels: new Uint32Array(length),
        lights: new Uint32Array(length),
      });
    }
  }
  return chunks;
}

function indexChunks(chunks: ChunkProtocol[]): Map<string, ChunkProtocol> {
  return new Map(chunks.map((chunk) => [`${chunk.x},${chunk.z}`, chunk]));
}

function fillInitialLight(chunks: ChunkProtocol[]): void {
  for (const chunk of chunks) {
    for (let index = 0; index < chunk.voxels.length; index += 1) {
      if ((chunk.voxels[index] & 0xffff) === LOCAL_BLOCK_IDS.air) {
        chunk.lights[index] = 15 << 12;
      }
    }
  }
}

function setVoxel(
  chunks: Map<string, ChunkProtocol>,
  x: number,
  y: number,
  z: number,
  id: number,
): void {
  if (y < 0 || y >= LOCAL_MAX_HEIGHT) return;
  if (!isInWorld(x, z)) return;
  const cx = Math.floor(x / LOCAL_CHUNK_SIZE);
  const cz = Math.floor(z / LOCAL_CHUNK_SIZE);
  const chunk = chunks.get(`${cx},${cz}`);
  if (chunk === undefined) return;
  setChunkVoxel(
    chunk,
    x - cx * LOCAL_CHUNK_SIZE,
    y,
    z - cz * LOCAL_CHUNK_SIZE,
    id,
  );
}

function setChunkVoxel(
  chunk: ChunkProtocol,
  lx: number,
  y: number,
  lz: number,
  id: number,
): void {
  chunk.voxels[voxelIndex(lx, y, lz)] = id;
}

function voxelIndex(lx: number, y: number, lz: number): number {
  return lx * LOCAL_MAX_HEIGHT * LOCAL_CHUNK_SIZE + y * LOCAL_CHUNK_SIZE + lz;
}

function isInWorld(x: number, z: number): boolean {
  return (
    x >= LOCAL_WORLD_MIN &&
    x <= LOCAL_WORLD_MAX &&
    z >= LOCAL_WORLD_MIN &&
    z <= LOCAL_WORLD_MAX
  );
}
