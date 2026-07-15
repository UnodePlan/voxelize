import { describe, expect, it } from "vitest";

import {
  LOCAL_EXTRACTION_REQUIRED_MS,
  LOCAL_INVENTORY_SLOTS,
  LOCAL_STACK_LIMIT,
  LOCAL_TOOL_HOTBAR_SLOTS,
  addResource,
  createInitialLocalGameState,
  dropInventorySlot,
  inventoryResourceCounts,
  isToolHotbarSlot,
  reduceLocalGameState,
  swapInventorySlots,
  voxelKey,
  type LocalGameState,
  type LocalInventorySlot,
} from "./state";

describe("local single-player state", () => {
  it("starts with a loading phase and twelve empty resource slots", () => {
    const state = createInitialLocalGameState();

    expect(state.phase).toBe("loading");
    expect(state.inventory).toHaveLength(LOCAL_INVENTORY_SLOTS);
    expect(state.inventory.every((slot) => slot === null)).toBe(true);
    expect(state.hint).toBe("controls");
    expect(state.inventoryOpen).toBe(false);
  });

  it("toggles Lab-style inventory with E and closes on Escape action", () => {
    let state = readyState();
    state = reduceLocalGameState(state, { type: "TOGGLE_INVENTORY" });
    expect(state.inventoryOpen).toBe(true);
    state = reduceLocalGameState(state, {
      type: "SET_INVENTORY_TAB",
      tab: "blocks",
    });
    expect(state.inventoryTab).toBe("blocks");
    state = reduceLocalGameState(state, { type: "CLOSE_INVENTORY" });
    expect(state.inventoryOpen).toBe(false);
  });

  it("reserves the first three hotbar slots for fixed tools", () => {
    expect(LOCAL_TOOL_HOTBAR_SLOTS).toBe(3);
    expect(isToolHotbarSlot(0)).toBe(true);
    expect(isToolHotbarSlot(2)).toBe(true);
    expect(isToolHotbarSlot(3)).toBe(false);
  });

  it("stacks the same resource before using a stable empty resource slot", () => {
    const inventory = emptyInventory();
    inventory[3] = { resource: "gold", quantity: LOCAL_STACK_LIMIT - 1 };
    inventory[5] = { resource: "dirt", quantity: 7 };

    const result = addResource(inventory, "gold", 3);

    expect(result.remainder).toBe(0);
    expect(result.inventory[0]).toBeNull();
    expect(result.inventory[1]).toBeNull();
    expect(result.inventory[2]).toBeNull();
    expect(result.inventory[3]).toEqual({
      resource: "gold",
      quantity: LOCAL_STACK_LIMIT,
    });
    expect(result.inventory[4]).toEqual({ resource: "gold", quantity: 2 });
    expect(result.inventory[5]).toEqual({ resource: "dirt", quantity: 7 });
    expect(inventory[3]).toEqual({
      resource: "gold",
      quantity: LOCAL_STACK_LIMIT - 1,
    });
  });

  it("never places mined resources into tool hotbar slots", () => {
    const inventory = emptyInventory();
    const result = addResource(inventory, "dirt", 1);

    expect(result.remainder).toBe(0);
    expect(result.inventory.slice(0, LOCAL_TOOL_HOTBAR_SLOTS)).toEqual([
      null,
      null,
      null,
    ]);
    expect(result.inventory[LOCAL_TOOL_HOTBAR_SLOTS]).toEqual({
      resource: "dirt",
      quantity: 1,
    });
  });

  it("returns overflow without silently destroying a full backpack resource", () => {
    const inventory = Array.from({ length: LOCAL_INVENTORY_SLOTS }, (_, index) =>
      isToolHotbarSlot(index)
        ? null
        : {
            resource: "dirt" as const,
            quantity: LOCAL_STACK_LIMIT,
          },
    );

    const result = addResource(inventory, "diamond", 4);

    expect(result.remainder).toBe(4);
    expect(result.inventory).toEqual(inventory);
  });

  it("drops one complete slot without changing the remaining slots", () => {
    const inventory = emptyInventory();
    inventory[3] = { resource: "diamond", quantity: 2 };

    const result = dropInventorySlot(inventory, 3);

    expect(result.dropped).toEqual({ resource: "diamond", quantity: 2 });
    expect(result.inventory[3]).toBeNull();
    expect(inventory[3]).toEqual({ resource: "diamond", quantity: 2 });
  });

  it("refuses drop and swap on fixed tool hotbar slots", () => {
    const inventory = emptyInventory();
    inventory[1] = { resource: "gold", quantity: 3 };
    inventory[4] = { resource: "dirt", quantity: 2 };

    expect(dropInventorySlot(inventory, 1).dropped).toBeNull();
    expect(swapInventorySlots(inventory, 1, 4)).toBe(inventory);
    expect(swapInventorySlots(inventory, 4, 0)).toBe(inventory);
  });

  it("swaps inventory slots for drag-and-drop rearrange", () => {
    const inventory = emptyInventory();
    inventory[3] = { resource: "dirt", quantity: 5 };
    inventory[5] = { resource: "gold", quantity: 1 };

    const swapped = swapInventorySlots(inventory, 3, 5);

    expect(swapped[3]).toEqual({ resource: "gold", quantity: 1 });
    expect(swapped[5]).toEqual({ resource: "dirt", quantity: 5 });
    expect(inventory[3]).toEqual({ resource: "dirt", quantity: 5 });

    let state = readyState();
    state = {
      ...state,
      inventory: swapped as LocalGameState["inventory"],
    };
    state = reduceLocalGameState(state, {
      type: "INVENTORY_SWAP",
      from: 5,
      to: 8,
    });
    expect(state.inventory[8]).toEqual({ resource: "dirt", quantity: 5 });
    expect(state.inventory[5]).toBeNull();
    expect(state.selectedSlot).toBe(8);
  });

  it("advances only the active mining target and caps at completion", () => {
    const target = [4, 8, -3] as const;
    let state = readyState();
    state = reduceLocalGameState(state, {
      type: "MINING_STARTED",
      target,
      resource: "gold",
      displayName: "黄金矿",
      requiredMs: 1_500,
    });
    state = reduceLocalGameState(state, {
      type: "MINING_ADVANCED",
      targetKey: voxelKey([5, 8, -3]),
      deltaMs: 800,
    });
    expect(state.mining?.elapsedMs).toBe(0);

    state = reduceLocalGameState(state, {
      type: "MINING_ADVANCED",
      targetKey: voxelKey(target),
      deltaMs: 2_000,
    });
    expect(state.mining).toMatchObject({
      elapsedMs: 1_500,
      requiredMs: 1_500,
      resource: "gold",
    });
  });

  it("resumes mining from persisted elapsedMs and accepts absolute ticks", () => {
    const target = [1, 2, 3] as const;
    let state = readyState();
    state = reduceLocalGameState(state, {
      type: "MINING_STARTED",
      target,
      resource: "stone",
      displayName: "岩石",
      requiredMs: 2_000,
      elapsedMs: 800,
    });
    expect(state.mining?.elapsedMs).toBe(800);

    state = reduceLocalGameState(state, {
      type: "MINING_ADVANCED",
      targetKey: voxelKey(target),
      deltaMs: 50,
      elapsedMs: 1_250,
    });
    expect(state.mining?.elapsedMs).toBe(1_250);

    state = reduceLocalGameState(state, { type: "MINING_CANCELLED" });
    expect(state.mining).toBeNull();
  });

  it("resets extraction progress when leaving the beacon", () => {
    let state = readyState();
    state = reduceLocalGameState(state, {
      type: "FRAME",
      deltaMs: LOCAL_EXTRACTION_REQUIRED_MS - 1,
      insideExtraction: true,
    });
    expect(state.extractionElapsedMs).toBe(LOCAL_EXTRACTION_REQUIRED_MS - 1);

    state = reduceLocalGameState(state, {
      type: "FRAME",
      deltaMs: 1,
      insideExtraction: false,
    });
    expect(state.phase).toBe("playing");
    expect(state.extractionElapsedMs).toBe(0);
  });

  it("extracts an empty backpack after three continuous seconds", () => {
    let state = readyState();
    state = reduceLocalGameState(state, {
      type: "FRAME",
      deltaMs: LOCAL_EXTRACTION_REQUIRED_MS,
      insideExtraction: true,
    });

    expect(state.phase).toBe("extracted");
    expect(state.result?.elapsedMs).toBe(LOCAL_EXTRACTION_REQUIRED_MS);
    expect(state.result?.resources).toMatchObject({
      dirt: 0,
      gold: 0,
      diamond: 0,
      stone: 0,
      planks: 0,
      leaves: 0,
      grass: 0,
    });

    const frozen = reduceLocalGameState(state, {
      type: "FRAME",
      deltaMs: 10_000,
      insideExtraction: true,
    });
    expect(frozen).toBe(state);
  });

  it("captures resource counts without calculating a score", () => {
    const inventory = emptyInventory();
    inventory[3] = { resource: "dirt", quantity: 4 };
    inventory[4] = { resource: "gold", quantity: 1 };
    inventory[5] = { resource: "diamond", quantity: 2 };
    inventory[6] = { resource: "stone", quantity: 9 };
    inventory[7] = { resource: "planks", quantity: 3 };

    expect(inventoryResourceCounts(inventory)).toMatchObject({
      dirt: 4,
      gold: 1,
      diamond: 2,
      stone: 9,
      planks: 3,
      grass: 0,
      leaves: 0,
    });
  });

  it("stacks structural drops like stone and planks", () => {
    const inventory = emptyInventory();
    const a = addResource(inventory, "stone", 10);
    const b = addResource(a.inventory, "planks", 2);
    const c = addResource(b.inventory, "leaves", 1);
    expect(c.remainder).toBe(0);
    expect(c.inventory[3]).toEqual({ resource: "stone", quantity: 10 });
    expect(c.inventory[4]).toEqual({ resource: "planks", quantity: 2 });
    expect(c.inventory[5]).toEqual({ resource: "leaves", quantity: 1 });
  });
});

function readyState(): LocalGameState {
  return reduceLocalGameState(createInitialLocalGameState(), {
    type: "WORLD_READY",
  });
}

function emptyInventory(): Array<LocalInventorySlot | null> {
  return Array.from({ length: LOCAL_INVENTORY_SLOTS }, () => null);
}
