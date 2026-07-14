import type { MessageProtocol } from "@voxelize/protocol";
import type { Texture } from "three";

import { createLocalBlocks } from "./blocks";
import {
  LOCAL_CHUNK_SIZE,
  LOCAL_MAX_CHUNK,
  LOCAL_MAX_HEIGHT,
  LOCAL_MIN_CHUNK,
  LOCAL_SUB_CHUNKS,
  createLocalQuarryMap,
  type LocalQuarryMap,
} from "./map";
import { createLocalTextureSources } from "./textures";

/** 首次 LOAD 与就绪判定：出生点周围 chunk 半径（与 runtime renderRadius 对齐） */
const LOCAL_SPAWN_CHUNK_RADIUS = 6;
/** 进入可玩所需的出生点邻域（小于 renderRadius，更快 ready） */
const LOCAL_READY_CHUNK_RADIUS = 2;

interface LocalWorldPort {
  applyTextureGroups(
    data: Array<{ groupName: string; source: Texture }>,
  ): Promise<unknown>;
  getChunkByCoords(
    cx: number,
    cz: number,
  ): { meshes: Map<number, unknown> } | undefined;
  initialize(): Promise<void>;
  onMessage(message: MessageProtocol): void;
  packets: MessageProtocol[];
  updateVoxel(
    x: number,
    y: number,
    z: number,
    type: number,
    options: { source: "server" },
  ): void;
}

type LocalTextureSource = { groupName: string; source: Texture };
type LocalTextureFactory = () =>
  | LocalTextureSource[]
  | Promise<LocalTextureSource[]>;

export class LocalWorldAdapter {
  readonly map: LocalQuarryMap;
  private readonly chunksByKey: Map<string, LocalQuarryMap["chunks"][number]>;

  constructor(
    map = createLocalQuarryMap(),
    private readonly textureFactory: LocalTextureFactory = () =>
      createLocalTextureSources(map.style.biome),
  ) {
    this.map = map;
    this.chunksByKey = new Map(
      map.chunks.map((chunk) => [`${chunk.x},${chunk.z}`, chunk]),
    );
  }

  async initializeWorld(world: LocalWorldPort): Promise<void> {
    world.onMessage({
      type: "INIT",
      json: createLocalInitData(this.map),
      entities: [],
    });
    await world.initialize();
    const textures = await this.textureFactory();
    try {
      await world.applyTextureGroups(textures);
    } finally {
      textures.forEach(({ source }) => source.dispose());
    }
    // 300×300 有 400 个 chunk：只预载出生点附近，其余按 World 请求增量供给
    world.onMessage({
      type: "LOAD",
      chunks: chunksNear(this.map, this.spawnChunk(), LOCAL_SPAWN_CHUNK_RADIUS),
    });
  }

  drainWorldPackets(world: LocalWorldPort): void {
    const packets = world.packets.splice(0, world.packets.length);
    for (const packet of packets) {
      if (packet.type !== "LOAD") continue;
      const requested = readRequestedChunks(packet.json);
      const chunks = requested
        .map(([cx, cz]) => this.chunksByKey.get(`${cx},${cz}`))
        .filter((chunk) => chunk !== undefined);
      if (chunks.length > 0) world.onMessage({ type: "LOAD", chunks });
    }
  }

  isWorldReady(world: LocalWorldPort): boolean {
    // 大地图不能等全图 400 chunk；出生点邻域有数据即可开玩
    const [scx, scz] = this.spawnChunk();
    for (let dx = -LOCAL_READY_CHUNK_RADIUS; dx <= LOCAL_READY_CHUNK_RADIUS; dx += 1) {
      for (
        let dz = -LOCAL_READY_CHUNK_RADIUS;
        dz <= LOCAL_READY_CHUNK_RADIUS;
        dz += 1
      ) {
        const cx = scx + dx;
        const cz = scz + dz;
        if (
          cx < LOCAL_MIN_CHUNK[0] ||
          cx > LOCAL_MAX_CHUNK[0] ||
          cz < LOCAL_MIN_CHUNK[1] ||
          cz > LOCAL_MAX_CHUNK[1]
        ) {
          continue;
        }
        if (world.getChunkByCoords(cx, cz) === undefined) return false;
      }
    }
    return true;
  }

  private spawnChunk(): readonly [number, number] {
    const [sx, , sz] = this.map.spawn;
    return [
      Math.floor(sx / LOCAL_CHUNK_SIZE),
      Math.floor(sz / LOCAL_CHUNK_SIZE),
    ];
  }

  applyServerVoxelUpdate(
    world: LocalWorldPort,
    voxel: readonly [number, number, number],
    type: number,
  ): void {
    world.updateVoxel(voxel[0], voxel[1], voxel[2], type, {
      source: "server",
    });
  }
}

export function createLocalInitData(map: LocalQuarryMap) {
  return {
    id: "single-player",
    blocks: createLocalBlocks(),
    items: [],
    options: {
      subChunks: LOCAL_SUB_CHUNKS,
      chunkSize: LOCAL_CHUNK_SIZE,
      maxHeight: LOCAL_MAX_HEIGHT,
      maxLightLevel: 15,
      minChunk: [...LOCAL_MIN_CHUNK],
      maxChunk: [...LOCAL_MAX_CHUNK],
      gravity: [0, -24.8, 0],
      minBounceImpulse: 0.5,
      doesTickTime: false,
      airDrag: 0.1,
      fluidDrag: 1.4,
      fluidDensity: 1,
      timePerDay: 24_000,
    },
    stats: { time: map.style.worldTime },
    savedPosition: [...map.spawn],
    savedDirection: [...map.initialDirection],
  };
}

function readRequestedChunks(value: unknown): Array<[number, number]> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return [];
  }
  const chunks = (value as { chunks?: unknown }).chunks;
  if (!Array.isArray(chunks)) return [];
  return chunks.filter(
    (chunk): chunk is [number, number] =>
      Array.isArray(chunk) &&
      chunk.length === 2 &&
      chunk.every((part) => Number.isInteger(part)),
  );
}

function chunksNear(
  map: LocalQuarryMap,
  center: readonly [number, number],
  radius: number,
) {
  const [scx, scz] = center;
  const r2 = radius * radius;
  return map.chunks.filter((chunk) => {
    const dx = chunk.x - scx;
    const dz = chunk.z - scz;
    return dx * dx + dz * dz <= r2;
  });
}
