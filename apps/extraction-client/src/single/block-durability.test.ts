import { describe, expect, it } from "vitest";

import { LocalBlockDurability } from "./block-durability";

describe("LocalBlockDurability", () => {
  it("accumulates damage across separate dig sessions", () => {
    const store = new LocalBlockDurability();
    const key = "3,4,5";
    expect(store.apply(key, 1001, 0.3)).toBeCloseTo(0.3, 5);
    // 松开后再挖
    expect(store.apply(key, 1001, 0.4)).toBeCloseTo(0.7, 5);
    expect(store.get(key, 1001)).toBeCloseTo(0.7, 5);
    expect(store.apply(key, 1001, 0.5)).toBe(1);
  });

  it("resets when block id changes at the same voxel", () => {
    const store = new LocalBlockDurability();
    const key = "1,2,3";
    store.apply(key, 1001, 0.6);
    expect(store.get(key, 1002)).toBe(0);
    expect(store.apply(key, 1002, 0.2)).toBeCloseTo(0.2, 5);
  });

  it("clear removes progress so the next dig starts fresh", () => {
    const store = new LocalBlockDurability();
    store.apply("0,0,0", 1, 0.9);
    store.clear("0,0,0");
    expect(store.get("0,0,0", 1)).toBe(0);
  });

  it("snapshot lists all damaged voxels for persistent crack overlays", () => {
    const store = new LocalBlockDurability();
    store.apply("1,2,3", 10, 0.4);
    store.apply("4,5,6", 11, 0.8);
    store.apply("7,8,9", 12, 0); // 无损伤不出现
    const snap = store.snapshot();
    expect(snap).toHaveLength(2);
    expect(snap).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          key: "1,2,3",
          voxel: [1, 2, 3],
          damage: 0.4,
          blockId: 10,
        }),
        expect.objectContaining({
          key: "4,5,6",
          voxel: [4, 5, 6],
          damage: 0.8,
          blockId: 11,
        }),
      ]),
    );
  });
});
