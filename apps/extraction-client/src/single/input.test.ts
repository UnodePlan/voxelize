import { describe, expect, it } from "vitest";

import { cycleInventorySlot } from "./input";

describe("local input helpers", () => {
  it("cycles all twelve inventory slots in both directions", () => {
    expect(cycleInventorySlot(0, -1)).toBe(11);
    expect(cycleInventorySlot(11, 1)).toBe(0);
    expect(cycleInventorySlot(5, 1)).toBe(6);
  });
});
