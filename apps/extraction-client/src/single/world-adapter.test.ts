import type { MessageProtocol } from "@voxelize/protocol";
import { Texture } from "three";
import { describe, expect, it, vi } from "vitest";

import {
  LOCAL_CHUNK_COUNT,
  LOCAL_MAX_CHUNK,
  LOCAL_MIN_CHUNK,
  LOCAL_SUB_CHUNKS,
  createLocalQuarryMap,
} from "./map";
import { LocalWorldAdapter, createLocalInitData } from "./world-adapter";

describe("local world adapter", () => {
  it("creates a fixed server-compatible init payload without network authority", () => {
    const map = createLocalQuarryMap();
    const init = createLocalInitData(map);

    expect(init.id).toBe("single-player");
    expect(init.options).toMatchObject({
      chunkSize: 16,
      maxHeight: 48,
      subChunks: 3,
      minChunk: [...LOCAL_MIN_CHUNK],
      maxChunk: [...LOCAL_MAX_CHUNK],
      doesTickTime: false,
    });
    expect(init.savedPosition).toEqual(map.spawn);
    expect(init).not.toHaveProperty("api");
    expect(init).not.toHaveProperty("socket");
  });

  it("initializes textures before loading all authored chunks", async () => {
    const port = new FakeWorldPort();
    const adapter = new LocalWorldAdapter(undefined, () =>
      Array.from({ length: 8 }, (_, index) => ({
        groupName: `test-${index}`,
        source: new Texture(),
      })),
    );

    await adapter.initializeWorld(port);

    expect(port.order.slice(0, 4)).toEqual([
      "message:INIT",
      "initialize",
      "textures",
      "message:LOAD",
    ]);
    // 只预载出生点附近；小地图可能覆盖全部 chunk
    const loaded = port.messages.at(-1)?.chunks?.length ?? 0;
    expect(loaded).toBeGreaterThan(0);
    expect(loaded).toBeLessThanOrEqual(LOCAL_CHUNK_COUNT);
    expect(port.textureGroups).toHaveLength(8);
  });

  it("answers in-bounds LOAD packets locally and discards transport output", () => {
    const port = new FakeWorldPort();
    const adapter = new LocalWorldAdapter();
    port.packets.push(
      {
        type: "LOAD",
        json: {
          chunks: [
            [0, 0],
            [99, 99],
          ],
        },
      },
      { type: "UNLOAD", json: { chunks: [[0, 0]] } },
    );

    adapter.drainWorldPackets(port);

    expect(port.packets).toEqual([]);
    expect(port.messages.at(-1)).toMatchObject({ type: "LOAD" });
    expect(port.messages.at(-1)?.chunks).toHaveLength(1);
    expect(port.messages.at(-1)?.chunks?.[0]).toMatchObject({ x: 0, z: 0 });
  });

  it("uses server-source local updates and detects spawn neighborhood ready", () => {
    const port = new FakeWorldPort();
    const adapter = new LocalWorldAdapter();
    // 只塞出生点邻域即可 ready（不必全图）
    const [sx, , sz] = adapter.map.spawn;
    const scx = Math.floor(sx / 16);
    const scz = Math.floor(sz / 16);
    for (let dx = -2; dx <= 2; dx += 1) {
      for (let dz = -2; dz <= 2; dz += 1) {
        port.loaded.set(`${scx + dx},${scz + dz}`, {
          meshes: new Map(
            Array.from({ length: LOCAL_SUB_CHUNKS }, (_, index) => [index, {}]),
          ),
        });
      }
    }

    adapter.applyServerVoxelUpdate(port, [1, 2, 3], 0);

    expect(port.updateVoxel).toHaveBeenCalledWith(1, 2, 3, 0, {
      source: "server",
    });
    expect(adapter.isWorldReady(port)).toBe(true);
  });

  it("waits until spawn neighborhood chunks have entered the real World", () => {
    const port = new FakeWorldPort();
    const adapter = new LocalWorldAdapter();
    const [sx, , sz] = adapter.map.spawn;
    const scx = Math.floor(sx / 16);
    const scz = Math.floor(sz / 16);
    for (let dx = -2; dx <= 2; dx += 1) {
      for (let dz = -2; dz <= 2; dz += 1) {
        port.loaded.set(`${scx + dx},${scz + dz}`, {
          meshes: new Map([[0, {}]]),
        });
      }
    }
    port.loaded.delete(`${scx},${scz}`);

    expect(adapter.isWorldReady(port)).toBe(false);
  });
});

class FakeWorldPort {
  readonly loaded = new Map<string, { meshes: Map<number, unknown> }>();
  readonly messages: MessageProtocol[] = [];
  readonly order: string[] = [];
  readonly packets: MessageProtocol[] = [];
  readonly textureGroups: unknown[] = [];
  readonly updateVoxel = vi.fn();

  onMessage(message: MessageProtocol): void {
    this.order.push(`message:${message.type}`);
    this.messages.push(message);
  }

  async initialize(): Promise<void> {
    this.order.push("initialize");
  }

  async applyTextureGroups(data: unknown[]): Promise<void> {
    this.order.push("textures");
    this.textureGroups.push(...data);
  }

  getChunkByCoords(cx: number, cz: number) {
    return this.loaded.get(`${cx},${cz}`);
  }
}
