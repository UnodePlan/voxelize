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

    // stone 1.5 · hand · canHarvest（单机始终可掉）→ 2.25s
    expect(miningDurationMs(stone, "empty")).toBe(2_250);
    // stone · iron pick speed 6 · canHarvest → 0.4s
    expect(miningDurationMs(stone, "pickaxe")).toBe(400);
    // 剑非镐 → 同空手
    expect(miningDurationMs(stone, "sword")).toBe(2_250);

    // cobble 2.0 · iron pick → 0.5s；手 3.0s
    expect(miningDurationMs(cobble, "pickaxe")).toBe(500);
    expect(miningDurationMs(cobble, "empty")).toBe(3_000);

    // diamond ore 3.0 · hand → 4.5s；铁镐 → 0.75s
    expect(miningDurationMs(ore, "empty")).toBe(4_500);
    expect(miningDurationMs(ore, "pickaxe")).toBe(750);
    expect(miningDurationMs(ore, "sword")).toBe(4_500);
  });

  it("uses iron pickaxe speed 6 only when preferred tool is pickaxe", () => {
    expect(destroySpeed("pickaxe", "pickaxe")).toBe(VANILLA_TOOL_SPEED.iron);
    expect(destroySpeed("pickaxe", "shovel")).toBe(VANILLA_TOOL_SPEED.hand);
    expect(destroySpeed("pickaxe", "axe")).toBe(VANILLA_TOOL_SPEED.hand);
    expect(destroySpeed("sword", "pickaxe")).toBe(VANILLA_TOOL_SPEED.hand);
    expect(destroySpeed("empty", "pickaxe")).toBe(VANILLA_TOOL_SPEED.hand);
  });

  it("drops every mineable block type regardless of held tool", () => {
    const ore = LOCAL_BLOCK_MINING[LOCAL_BLOCK_IDS.gold];
    const dirt = LOCAL_BLOCK_MINING[LOCAL_BLOCK_IDS.dirt];
    const timber = LOCAL_BLOCK_MINING[LOCAL_BLOCK_IDS.weatheredTimber];
    const stone = LOCAL_BLOCK_MINING[LOCAL_BLOCK_IDS.quarryStone];
    const leaves = LOCAL_BLOCK_MINING[LOCAL_BLOCK_IDS.leaves];

    // 单机：工具不挡掉落；镐仍是石/矿最佳工具（速度）
    expect(canHarvestWith("empty", ore)).toBe(true);
    expect(canHarvestWith("pickaxe", ore)).toBe(true);
    expect(harvestDrop(ore.drop, "empty", ore)).toBe("gold");
    expect(harvestDrop(ore.drop, "pickaxe", ore)).toBe("gold");

    expect(harvestDrop(dirt.drop, "empty", dirt)).toBe("dirt");
    expect(harvestDrop(timber.drop, "empty", timber)).toBe("planks");
    expect(harvestDrop(stone.drop, "sword", stone)).toBe("stone");
    expect(harvestDrop(leaves.drop, "empty", leaves)).toBe("leaves");
  });

  it("uses plank hardness 2.0 for timber with hand = 3s", () => {
    const wood = LOCAL_BLOCK_MINING[LOCAL_BLOCK_IDS.weatheredTimber];
    // 无斧：手/镐/剑均为 speed 1、可收获 → 3.0s
    expect(miningDurationMs(wood, "empty")).toBe(3_000);
    expect(miningDurationMs(wood, "pickaxe")).toBe(3_000);
    expect(miningDurationMs(wood, "sword")).toBe(3_000);
  });

  it("exposes MC hardness and non-null drops on all mineable profiles", () => {
    expect(getBlockMiningProfile(LOCAL_BLOCK_IDS.dirt)?.hardness).toBe(0.5);
    expect(getBlockMiningProfile(LOCAL_BLOCK_IDS.diamond)?.hardness).toBe(3.0);
    expect(getBlockMiningProfile(LOCAL_BLOCK_IDS.quarryStone)?.drop).toBe(
      "stone",
    );
    for (const profile of Object.values(LOCAL_BLOCK_MINING)) {
      expect(profile.drop).toBeTruthy();
    }
  });
});
