import { describe, expect, it } from "vitest";

import { resourceStack } from "./loot-entity";

describe("resourceStack", () => {
  it("orders by rarity and caps at three", () => {
    expect(resourceStack(undefined)).toEqual(["dirt"]);
    expect(resourceStack({ dirt: 1, gold: 2, diamond: 1 })).toEqual([
      "diamond",
      "gold",
      "dirt",
    ]);
    expect(resourceStack({ dirt: 0, gold: 3, diamond: 0 })).toEqual(["gold"]);
  });
});
