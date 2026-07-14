import { describe, expect, it } from "vitest";

import {
  LOCAL_BLOCK_IDS,
  LOCAL_BLOCK_MINING,
  LOCAL_MINEABLE_BLOCKS,
  createLocalBlocks,
  isBlockMineable,
} from "./blocks";

describe("local block registry", () => {
  it("builds a complete air block and textured block definitions", () => {
    const blocks = createLocalBlocks();

    expect(Object.keys(blocks)).toHaveLength(11);
    expect(blocks.Leaves.isSeeThrough).toBe(true);
    expect(blocks.Water).toBeUndefined();
    expect(blocks.Grass.faces).toHaveLength(6);
    expect(
      blocks.Grass.faces.find((face) => face.name === "py")?.textureGroup,
    ).toBe("single-grass-top");
    expect(blocks.Air).toMatchObject({
      id: LOCAL_BLOCK_IDS.air,
      isEmpty: true,
      isPassable: true,
      isTransparent: [true, true, true, true, true, true],
    });
    expect(blocks.Air.faces).toEqual([]);
    expect(blocks["Quarry Stone"].faces).toHaveLength(6);
    expect(blocks["Quarry Stone"].aabbs).toEqual([
      { minX: 0, minY: 0, minZ: 0, maxX: 1, maxY: 1, maxZ: 1 },
    ]);
  });

  it("allows mining registered terrain blocks but keeps bedrock and beacon solid", () => {
    const mineableIds = Object.keys(LOCAL_BLOCK_MINING).map(Number).sort();
    expect(mineableIds).toEqual(
      [
        LOCAL_BLOCK_IDS.quarryStone,
        LOCAL_BLOCK_IDS.paleStone,
        LOCAL_BLOCK_IDS.weatheredTimber,
        LOCAL_BLOCK_IDS.grass,
        LOCAL_BLOCK_IDS.dirt,
        LOCAL_BLOCK_IDS.gold,
        LOCAL_BLOCK_IDS.diamond,
        LOCAL_BLOCK_IDS.leaves,
      ].sort(),
    );
    expect(isBlockMineable(LOCAL_BLOCK_IDS.bedrock)).toBe(false);
    expect(isBlockMineable(LOCAL_BLOCK_IDS.extractionMarker)).toBe(false);

    // 兼容旧表：仅资源掉落方块
    expect(Object.keys(LOCAL_MINEABLE_BLOCKS).map(Number).sort()).toEqual(
      [
        LOCAL_BLOCK_IDS.dirt,
        LOCAL_BLOCK_IDS.grass,
        LOCAL_BLOCK_IDS.gold,
        LOCAL_BLOCK_IDS.diamond,
      ].sort(),
    );
    // 兼容表：泥土空手原版 0.75s
    expect(LOCAL_MINEABLE_BLOCKS[LOCAL_BLOCK_IDS.dirt]).toMatchObject({
      resource: "dirt",
      miningDurationMs: 750,
    });
  });

  it("assigns one stable atlas range to all faces in a texture group", () => {
    const blocks = createLocalBlocks();
    const ranges = blocks.Gold.faces.map((face) => face.range);

    expect(ranges.every((range) => range.startU === ranges[0].startU)).toBe(
      true,
    );
    expect(ranges[0].startU).toBeGreaterThanOrEqual(0);
    expect(ranges[0].endV).toBeLessThanOrEqual(1);
  });
});
