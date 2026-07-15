import { describe, expect, it } from "vitest";

import { mcCharacterMetrics } from "./mc-character";

describe("mcCharacterMetrics", () => {
  it("uses MC pixel proportions (8/12/4)", () => {
    const m = mcCharacterMetrics(1);
    expect(m.unit).toBeCloseTo(1 / 16, 10);
    expect(m.head.width).toBeCloseTo(0.5, 5);
    expect(m.body.height).toBeCloseTo(0.75, 5);
    expect(m.limb.height).toBeCloseTo(0.75, 5);
    expect(m.totalHeight).toBeCloseTo(2, 5);
    expect(m.eyeHeight).toBeCloseTo(1.75, 5);
  });

  it("scales linearly", () => {
    const m = mcCharacterMetrics(0.5);
    expect(m.totalHeight).toBeCloseTo(1, 5);
  });
});
