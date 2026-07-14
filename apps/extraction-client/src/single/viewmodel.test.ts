import { describe, expect, it } from "vitest";

import { heldToolFromSlot } from "./viewmodel";

describe("heldToolFromSlot", () => {
  it("maps hotbar 0/1/2 to empty/pickaxe/sword", () => {
    expect(heldToolFromSlot(0)).toBe("empty");
    expect(heldToolFromSlot(1)).toBe("pickaxe");
    expect(heldToolFromSlot(2)).toBe("sword");
  });

  it("treats resource slots as empty hand in first person", () => {
    expect(heldToolFromSlot(3)).toBe("empty");
    expect(heldToolFromSlot(11)).toBe("empty");
  });
});
