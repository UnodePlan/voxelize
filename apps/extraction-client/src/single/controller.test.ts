import { Vector3 } from "three";
import { describe, expect, it } from "vitest";

import {
  hasFallenOutOfTerrain,
  isInsideExtraction,
  pickRandomSkyDrop,
} from "./gameplay-math";

describe("local extraction zone", () => {
  const zone = { center: [0.5, 25, 24.5] as const, radius: 2.1 };

  it("accepts the player center inside the beacon cylinder", () => {
    expect(isInsideExtraction(new Vector3(0.5, 26.6, 24.5), zone)).toBe(true);
    expect(isInsideExtraction(new Vector3(3, 26.6, 24.5), zone)).toBe(false);
  });

  it("rejects positions on another vertical layer", () => {
    expect(isInsideExtraction(new Vector3(0.5, 18, 24.5), zone)).toBe(false);
  });
});

describe("void fall sky drop", () => {
  const worldMin = -30;
  const worldMax = 29;

  it("detects falling below the void line", () => {
    expect(
      hasFallenOutOfTerrain(new Vector3(0, -3, 0), worldMin, worldMax),
    ).toBe(true);
    expect(
      hasFallenOutOfTerrain(new Vector3(0, 16, 0), worldMin, worldMax),
    ).toBe(false);
  });

  it("detects low positions far outside the map", () => {
    expect(
      hasFallenOutOfTerrain(new Vector3(80, 2, 0), worldMin, worldMax),
    ).toBe(true);
    expect(
      hasFallenOutOfTerrain(new Vector3(80, 20, 0), worldMin, worldMax),
    ).toBe(false);
  });

  it("picks a high drop inside the map above the surface", () => {
    const surfaceY = () => 12;
    const drop = pickRandomSkyDrop(worldMin, worldMax, surfaceY, {
      dropHeight: 28,
      maxY: 46,
      random: () => 0.5,
    });
    expect(drop[0]).toBeGreaterThanOrEqual(worldMin);
    expect(drop[0]).toBeLessThanOrEqual(worldMax + 1);
    expect(drop[2]).toBeGreaterThanOrEqual(worldMin);
    expect(drop[2]).toBeLessThanOrEqual(worldMax + 1);
    expect(drop[1]).toBe(12 + 28);
  });
});
