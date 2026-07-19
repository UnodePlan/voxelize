import { describe, expect, it } from "vitest";

import {
  heldContentEquals,
  heldContentFromSlot,
  heldContentKey,
} from "./held-content";
import type { LocalInventorySlot } from "./state";

function emptyInventory(): Array<LocalInventorySlot | null> {
  return Array.from({ length: 12 }, () => null);
}

describe("heldContentFromSlot", () => {
  it("maps tool hotbar slots", () => {
    const inv = emptyInventory();
    expect(heldContentFromSlot(0, inv)).toEqual({ kind: "empty" });
    expect(heldContentFromSlot(1, inv)).toEqual({
      kind: "tool",
      tool: "pickaxe",
    });
    expect(heldContentFromSlot(2, inv)).toEqual({
      kind: "tool",
      tool: "sword",
    });
  });

  it("maps resource slots with quantity to held blocks", () => {
    const inv = emptyInventory();
    inv[3] = { resource: "dirt", quantity: 4 };
    inv[5] = { resource: "diamond", quantity: 1 };
    expect(heldContentFromSlot(3, inv)).toEqual({
      kind: "block",
      resource: "dirt",
    });
    expect(heldContentFromSlot(5, inv)).toEqual({
      kind: "block",
      resource: "diamond",
    });
    expect(heldContentFromSlot(4, inv)).toEqual({ kind: "empty" });
  });

  it("treats zero-quantity and empty resource slots as empty hand", () => {
    const inv = emptyInventory();
    inv[3] = { resource: "stone", quantity: 0 };
    expect(heldContentFromSlot(3, inv)).toEqual({ kind: "empty" });
  });
});

describe("heldContentKey", () => {
  it("distinguishes tool and block contents", () => {
    expect(heldContentKey({ kind: "empty" })).toBe("empty");
    expect(heldContentKey({ kind: "tool", tool: "sword" })).toBe("tool:sword");
    expect(heldContentKey({ kind: "block", resource: "gold" })).toBe(
      "block:gold",
    );
    expect(
      heldContentEquals(
        { kind: "block", resource: "dirt" },
        { kind: "block", resource: "dirt" },
      ),
    ).toBe(true);
    expect(
      heldContentEquals(
        { kind: "block", resource: "dirt" },
        { kind: "tool", tool: "pickaxe" },
      ),
    ).toBe(false);
  });
});
