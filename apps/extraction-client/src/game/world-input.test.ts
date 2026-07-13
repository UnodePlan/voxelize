import { Vector3 } from "three";
import { describe, expect, it } from "vitest";

import { isMovementDue, isPeerHitCloser } from "./world-input";

describe("world input targeting", () => {
  it("attacks a peer only when it is in front of the targeted voxel", () => {
    const eye = new Vector3(0.5, 0.5, 0.5);
    expect(isPeerHitCloser(1.5, eye, [0, 0, 3])).toBe(true);
    expect(isPeerHitCloser(2.8, eye, [0, 0, 3])).toBe(false);
  });

  it("attacks a visible peer when no voxel is targeted", () => {
    expect(isPeerHitCloser(2.9, new Vector3(), null)).toBe(true);
  });

  it("caps changed movement and camera direction updates at twenty hertz", () => {
    expect(isMovementDue(49, 0)).toBe(false);
    expect(isMovementDue(50, 0)).toBe(true);
    expect(isMovementDue(1, 0, true)).toBe(true);
  });
});
