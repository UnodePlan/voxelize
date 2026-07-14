import { describe, expect, it } from "vitest";

import {
  LOCAL_EXTRACTION_REQUIRED_MS,
  LOCAL_INVENTORY_SLOTS,
  LOCAL_STACK_LIMIT,
  addResource,
  createInitialLocalGameState,
  dropInventorySlot,
  inventoryResourceCounts,
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

  it("stacks the same resource before using a stable empty slot", () => {
    const inventory = emptyInventory();
    inventory[0] = { resource: "gold", quantity: LOCAL_STACK_LIMIT - 1 };
    inventory[2] = { resource: "dirt", quantity: 7 };

    const result = addResource(inventory, "gold", 3);

    expect(result.remainder).toBe(0);
    expect(result.inventory[0]).toEqual({
      resource: "gold",
      quantity: LOCAL_STACK_LIMIT,
    });
    expect(result.inventory[1]).toEqual({ resource: "gold", quantity: 2 });
    expect(result.inventory[2]).toEqual({ resource: "dirt", quantity: 7 });
    expect(inventory[0]).toEqual({
      resource: "gold",
      quantity: LOCAL_STACK_LIMIT - 1,
    });
  });

  it("returns overflow without silently destroying a full backpack resource", () => {
    const inventory = Array.from({ length: LOCAL_INVENTORY_SLOTS }, () => ({
      resource: "dirt" as const,
      quantity: LOCAL_STACK_LIMIT,
    }));

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

  it("swaps inventory slots for drag-and-drop rearrange", () => {
    const inventory = emptyInventory();
    inventory[0] = { resource: "dirt", quantity: 5 };
    inventory[2] = { resource: "gold", quantity: 1 };

    const swapped = swapInventorySlots(inventory, 0, 2);

    expect(swapped[0]).toEqual({ resource: "gold", quantity: 1 });
    expect(swapped[2]).toEqual({ resource: "dirt", quantity: 5 });
    expect(inventory[0]).toEqual({ resource: "dirt", quantity: 5 });

    let state = readyState();
    state = {
      ...state,
      inventory: swapped as LocalGameState["inventory"],
    };
    state = reduceLocalGameState(state, {
      type: "INVENTORY_SWAP",
      from: 2,
      to: 5,
    });
    expect(state.inventory[5]).toEqual({ resource: "dirt", quantity: 5 });
    expect(state.inventory[2]).toBeNull();
    expect(state.selectedSlot).toBe(5);
  });

  it("advances only the active mining target and caps at completion", () => {
    const target = [4, 8, -3] as const;
    let state = readyState();
    state = reduceLocalGameState(state, {
      type: "MINING_STARTED",
      target,
      resource: "gold",
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
    expect(state.result).toEqual({
      elapsedMs: LOCAL_EXTRACTION_REQUIRED_MS,
      resources: { dirt: 0, gold: 0, diamond: 0 },
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
    inventory[0] = { resource: "dirt", quantity: 4 };
    inventory[1] = { resource: "gold", quantity: 1 };
    inventory[2] = { resource: "diamond", quantity: 2 };

    expect(inventoryResourceCounts(inventory)).toEqual({
      dirt: 4,
      gold: 1,
      diamond: 2,
    });
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
