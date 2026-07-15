import { describe, expect, it } from "vitest";

import { MC_SKIN_OVERLAY_UV, MC_SKIN_UV } from "./mc-skin";

describe("MC_SKIN_UV", () => {
  it("uses classic 64×64 head front at (8,8) 8×8", () => {
    expect(MC_SKIN_UV.head.front).toEqual([8, 8, 8, 8]);
    expect(MC_SKIN_UV.head.top).toEqual([8, 0, 8, 8]);
  });

  it("places torso and limbs in standard regions", () => {
    expect(MC_SKIN_UV.body.front).toEqual([20, 20, 8, 12]);
    expect(MC_SKIN_UV.rightArm.front).toEqual([44, 20, 4, 12]);
    expect(MC_SKIN_UV.leftArm.front).toEqual([36, 52, 4, 12]);
    expect(MC_SKIN_UV.rightLeg.front).toEqual([4, 20, 4, 12]);
    expect(MC_SKIN_UV.leftLeg.front).toEqual([20, 52, 4, 12]);
  });

  it("defines head helm overlay next to base head", () => {
    expect(MC_SKIN_OVERLAY_UV.head.front).toEqual([40, 8, 8, 8]);
    expect(MC_SKIN_OVERLAY_UV.body.front).toEqual([20, 36, 8, 12]);
  });

  it("keeps all rects inside 64×64", () => {
    const tables = [MC_SKIN_UV, MC_SKIN_OVERLAY_UV] as const;
    for (const table of tables) {
      for (const faces of Object.values(table)) {
        for (const rect of Object.values(faces) as Array<
          readonly [number, number, number, number]
        >) {
          const [x, y, w, h] = rect;
          expect(x).toBeGreaterThanOrEqual(0);
          expect(y).toBeGreaterThanOrEqual(0);
          expect(x + w).toBeLessThanOrEqual(64);
          expect(y + h).toBeLessThanOrEqual(64);
        }
      }
    }
  });
});
