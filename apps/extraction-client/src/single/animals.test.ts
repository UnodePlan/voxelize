import { describe, expect, it } from "vitest";

import { planBiomeAnimals } from "./animals";
import { LOCAL_MAP_STYLES } from "./map-style";

describe("local biome animals", () => {
  it("plans a stable non-empty set for each biome", () => {
    const biomes = new Set(LOCAL_MAP_STYLES.map((s) => s.biome));
    for (const biome of biomes) {
      const a = planBiomeAnimals(42, biome, () => 16);
      const b = planBiomeAnimals(42, biome, () => 16);
      expect(a.length).toBeGreaterThan(0);
      expect(a.map((p) => `${p.kind}:${p.x},${p.z}`)).toEqual(
        b.map((p) => `${p.kind}:${p.x},${p.z}`),
      );
      // 每种动物都有中文名与调色
      for (const plan of a) {
        expect(plan.label.length).toBeGreaterThan(0);
        expect(plan.palette.body.startsWith("#")).toBe(true);
      }
    }
  });

  it("gives spring soft animals and desert arid ones", () => {
    const spring = planBiomeAnimals(7, "spring", () => 16);
    const desert = planBiomeAnimals(7, "desert", () => 16);
    expect(spring.some((p) => p.kind === "rabbit" || p.kind === "duck")).toBe(
      true,
    );
    expect(desert.every((p) => p.kind === "camel" || p.kind === "scavenger")).toBe(
      true,
    );
  });

  it("respects blocked cells when placing animals", () => {
    const blocked = new Set(["0,0", "1,1"]);
    const plans = planBiomeAnimals(
      99,
      "meadow",
      () => 16,
      (x, z) => blocked.has(`${x},${z}`) || Math.abs(x) + Math.abs(z) < 20,
    );
    // 大面积封锁时可能变少，但不该落在 blocked 精确点
    for (const p of plans) {
      expect(blocked.has(`${Math.floor(p.x)},${Math.floor(p.z)}`)).toBe(false);
    }
  });
});
