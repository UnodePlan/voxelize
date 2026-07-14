import { describe, expect, it } from "vitest";

import {
  getBlockMiningProfile,
  isBlockMineable,
  LOCAL_BLOCK_IDS,
  LOCAL_BLOCK_MINING,
} from "./blocks";
import {
  canHarvestWith,
  destroySpeed,
  harvestDrop,
  miningDurationMs,
  VANILLA_TOOL_SPEED,
} from "./mining";

describe("vanilla mining formula", () => {
  it("registers structural and resource blocks as mineable, not bedrock", () => {
    expect(isBlockMineable(LOCAL_BLOCK_IDS.dirt)).toBe(true);
    expect(isBlockMineable(LOCAL_BLOCK_IDS.quarryStone)).toBe(true);
    expect(isBlockMineable(LOCAL_BLOCK_IDS.weatheredTimber)).toBe(true);
    expect(isBlockMineable(LOCAL_BLOCK_IDS.grass)).toBe(true);
    expect(isBlockMineable(LOCAL_BLOCK_IDS.bedrock)).toBe(false);
    expect(isBlockMineable(LOCAL_BLOCK_IDS.extractionMarker)).toBe(false);
    expect(isBlockMineable(LOCAL_BLOCK_IDS.air)).toBe(false);
  });

  it("matches wiki times for dirt, stone, and diamond ore", () => {
    const dirt = LOCAL_BLOCK_MINING[LOCAL_BLOCK_IDS.dirt];
    const stone = LOCAL_BLOCK_MINING[LOCAL_BLOCK_IDS.paleStone];
    const ore = LOCAL_BLOCK_MINING[LOCAL_BLOCK_IDS.diamond];
    const cobble = LOCAL_BLOCK_MINING[LOCAL_BLOCK_IDS.quarryStone];

    // dirt 0.5 · hand · canHarvest · speed 1 → 0.75s
    expect(miningDurationMs(dirt, "empty")).toBe(750);
    // 无铲：铁镐对泥土不是最佳工具 → 同空手 0.75s
    expect(miningDurationMs(dirt, "pickaxe")).toBe(750);

    // stone 1.5 · hand · !canHarvest → 7.5s
    expect(miningDurationMs(stone, "empty")).toBe(7_500);
    // stone · iron pick speed 6 · canHarvest → 0.4s
    expect(miningDurationMs(stone, "pickaxe")).toBe(400);
    // 剑非镐 → 同空手慢挖
    expect(miningDurationMs(stone, "sword")).toBe(7_500);

    // cobble 2.0 · iron pick → 0.5s
    expect(miningDurationMs(cobble, "pickaxe")).toBe(500);
    expect(miningDurationMs(cobble, "empty")).toBe(10_000);

    // diamond ore 3.0 · hand → 15s；铁镐 → 0.75s
    expect(miningDurationMs(ore, "empty")).toBe(15_000);
    expect(miningDurationMs(ore, "pickaxe")).toBe(750);
    expect(miningDurationMs(ore, "sword")).toBe(15_000);
  });

  it("uses iron pickaxe speed 6 only when preferred tool is pickaxe", () => {
    expect(destroySpeed("pickaxe", "pickaxe")).toBe(VANILLA_TOOL_SPEED.iron);
    expect(destroySpeed("pickaxe", "shovel")).toBe(VANILLA_TOOL_SPEED.hand);
    expect(destroySpeed("pickaxe", "axe")).toBe(VANILLA_TOOL_SPEED.hand);
    expect(destroySpeed("sword", "pickaxe")).toBe(VANILLA_TOOL_SPEED.hand);
    expect(destroySpeed("empty", "pickaxe")).toBe(VANILLA_TOOL_SPEED.hand);
  });

  it("only drops when canHarvest (pickaxe required for ore/stone)", () => {
    const ore = LOCAL_BLOCK_MINING[LOCAL_BLOCK_IDS.gold];
    const dirt = LOCAL_BLOCK_MINING[LOCAL_BLOCK_IDS.dirt];
    const timber = LOCAL_BLOCK_MINING[LOCAL_BLOCK_IDS.weatheredTimber];

    expect(canHarvestWith("empty", ore)).toBe(false);
    expect(canHarvestWith("sword", ore)).toBe(false);
    expect(canHarvestWith("pickaxe", ore)).toBe(true);
    expect(harvestDrop(ore.drop, "empty", ore)).toBeNull();
    expect(harvestDrop(ore.drop, "pickaxe", ore)).toBe("gold");

    // 土不要求正确工具
    expect(canHarvestWith("empty", dirt)).toBe(true);
    expect(harvestDrop(dirt.drop, "empty", dirt)).toBe("dirt");
    expect(harvestDrop(dirt.drop, "sword", dirt)).toBe("dirt");

    // 结构木：可挖穿但不掉落
    expect(canHarvestWith("empty", timber)).toBe(true);
    expect(harvestDrop(timber.drop, "empty", timber)).toBeNull();
  });

  it("uses plank hardness 2.0 for timber with hand = 3s", () => {
    const wood = LOCAL_BLOCK_MINING[LOCAL_BLOCK_IDS.weatheredTimber];
    // 无斧：手/镐/剑均为 speed 1、可收获 → 3.0s
    expect(miningDurationMs(wood, "empty")).toBe(3_000);
    expect(miningDurationMs(wood, "pickaxe")).toBe(3_000);
    expect(miningDurationMs(wood, "sword")).toBe(3_000);
  });

  it("exposes MC hardness on profiles", () => {
    expect(getBlockMiningProfile(LOCAL_BLOCK_IDS.dirt)?.hardness).toBe(0.5);
    expect(getBlockMiningProfile(LOCAL_BLOCK_IDS.diamond)?.hardness).toBe(3.0);
    expect(getBlockMiningProfile(LOCAL_BLOCK_IDS.quarryStone)?.drop).toBeNull();
  });
});
