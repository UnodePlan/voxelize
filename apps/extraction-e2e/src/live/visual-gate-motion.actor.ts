import { describe, expect, it } from "vitest";

import { isCanvasDifferenceStable } from "./visual-gate-canvas";
import {
  movementDifferenceErrors,
  movementInputErrors,
  type MovementInputEvidence,
} from "./visual-gate-motion-rules";
import type { CanvasDifference } from "./visual-gate-webgl";

function difference(
  changedRatio: number,
  meanChannelDelta: number,
): CanvasDifference {
  return { changedRatio, meanChannelDelta, sampledPixels: 5_184 };
}

function validInput(): MovementInputEvidence {
  return {
    authoritativeDisplacements: [2.5],
    keyEvents: [
      { code: "KeyW", isTrusted: true, locked: true, type: "keydown" },
      { code: "KeyW", isTrusted: true, locked: true, type: "keyup" },
    ],
    mouseEvents: [
      { isTrusted: true, locked: true, movementX: 0, movementY: 80 },
    ],
    outgoingDirections: [
      [0, 0, -1],
      [0, -0.2, -0.98],
    ],
    outgoingForwardValues: [0, 1, 1, 0],
    pointerLockChanges: [true],
  };
}

describe("visual gate motion rules", () => {
  it("requires two fingerprints to stay inside the strict stability bounds", () => {
    expect(isCanvasDifferenceStable(difference(0.003, 0.75))).toBe(true);
    expect(isCanvasDifferenceStable(difference(0.0031, 0.75))).toBe(false);
    expect(isCanvasDifferenceStable(difference(0.003, 0.751))).toBe(false);
  });

  it("accepts trusted locked KeyW down/up and forward then stopped frames", () => {
    expect(movementInputErrors(validInput())).toEqual([]);
  });

  it("rejects an untrusted keyup and a stop frame that precedes forward", () => {
    const input = validInput();
    input.keyEvents[1] = {
      code: "KeyW",
      isTrusted: false,
      locked: true,
      type: "keyup",
    };
    input.outgoingForwardValues = [0, 1, 1];

    expect(movementInputErrors(input)).toEqual([
      "trusted locked KeyW up was not observed",
      "stopped movement frame was not observed",
    ]);
  });

  it("requires trusted look, a changed direction and authoritative displacement", () => {
    const input = validInput();
    input.mouseEvents = [];
    input.outgoingDirections = [[0, 0, -1]];
    input.authoritativeDisplacements = [0.99];

    expect(movementInputErrors(input)).toEqual([
      "trusted locked mouse look was not observed",
      "outgoing look direction did not change",
      "authoritative peer position did not move one block",
    ]);
  });

  it("keeps a strict gap between stable noise and meaningful movement", () => {
    expect(movementDifferenceErrors(difference(0.01, 2))).toEqual([]);
    expect(movementDifferenceErrors(difference(0.009, 1.99))).toEqual([
      "changed pixel ratio is too low",
      "mean channel delta is too low",
    ]);
  });
});
