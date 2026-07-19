import { describe, expect, it } from "vitest";

import {
  LOCAL_BLOCK_IDS,
  LOCAL_BLOCK_MINING,
  LOCAL_MINEABLE_BLOCKS,
  createLocalBlocks,
  isBlockMineable,
  placeBlockIdForResource,
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

  it("maps inventory resources to placeable block ids", () => {
    expect(placeBlockIdForResource("dirt")).toBe(LOCAL_BLOCK_IDS.dirt);
    expect(placeBlockIdForResource("grass")).toBe(LOCAL_BLOCK_IDS.grass);
    expect(placeBlockIdForResource("stone")).toBe(LOCAL_BLOCK_IDS.paleStone);
    expect(placeBlockIdForResource("planks")).toBe(
      LOCAL_BLOCK_IDS.weatheredTimber,
    );
    expect(placeBlockIdForResource("leaves")).toBe(LOCAL_BLOCK_IDS.leaves);
    expect(placeBlockIdForResource("gold")).toBe(LOCAL_BLOCK_IDS.gold);
    expect(placeBlockIdForResource("diamond")).toBe(LOCAL_BLOCK_IDS.diamond);
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

    // 全部可挖方块均有掉落资源
    expect(Object.keys(LOCAL_MINEABLE_BLOCKS).map(Number).sort()).toEqual(
      mineableIds,
    );
    expect(LOCAL_MINEABLE_BLOCKS[LOCAL_BLOCK_IDS.dirt]).toMatchObject({
      resource: "dirt",
      miningDurationMs: 750,
    });
    expect(LOCAL_MINEABLE_BLOCKS[LOCAL_BLOCK_IDS.quarryStone]).toMatchObject({
      resource: "stone",
    });
    expect(
      LOCAL_MINEABLE_BLOCKS[LOCAL_BLOCK_IDS.weatheredTimber],
    ).toMatchObject({ resource: "planks" });
    expect(LOCAL_MINEABLE_BLOCKS[LOCAL_BLOCK_IDS.leaves]).toMatchObject({
      resource: "leaves",
    });
    expect(LOCAL_MINEABLE_BLOCKS[LOCAL_BLOCK_IDS.grass]).toMatchObject({
      resource: "grass",
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
