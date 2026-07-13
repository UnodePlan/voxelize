import type { CanvasDifference } from "./visual-gate-webgl";

const MIN_MOVEMENT_CHANGED_RATIO = 0.01;
const MIN_MOVEMENT_MEAN_CHANNEL_DELTA = 2;
const MIN_AUTHORITATIVE_DISPLACEMENT = 1;
const MIN_LOOK_DIRECTION_DELTA = 0.05;
const MIN_TRUSTED_MOUSE_DELTA = 8;

export interface MovementInputEvidence {
  authoritativeDisplacements: number[];
  keyEvents: Array<{
    code: string;
    isTrusted: boolean;
    locked: boolean;
    type: "keydown" | "keyup";
  }>;
  mouseEvents: Array<{
    isTrusted: boolean;
    locked: boolean;
    movementX: number;
    movementY: number;
  }>;
  outgoingDirections: Array<[number, number, number]>;
  outgoingForwardValues: number[];
  pointerLockChanges: boolean[];
}

export function movementInputErrors(evidence: MovementInputEvidence): string[] {
  const errors: string[] = [];
  const keyDownIndex = evidence.keyEvents.findIndex(
    (event) =>
      event.type === "keydown" && event.isTrusted === true && event.locked,
  );
  const keyUpIndex = evidence.keyEvents.findIndex(
    (event, index) =>
      index > keyDownIndex &&
      event.type === "keyup" &&
      event.isTrusted === true &&
      event.locked,
  );
  if (keyDownIndex < 0)
    errors.push("trusted locked KeyW down was not observed");
  if (keyUpIndex < 0) errors.push("trusted locked KeyW up was not observed");
  if (!evidence.pointerLockChanges.includes(true)) {
    errors.push("pointer lock acquisition was not observed");
  }
  const forwardIndex = evidence.outgoingForwardValues.indexOf(1);
  const stoppedIndex = evidence.outgoingForwardValues.findIndex(
    (value, index) => index > forwardIndex && value === 0,
  );
  if (forwardIndex < 0) errors.push("forward movement frame was not observed");
  if (stoppedIndex < 0) errors.push("stopped movement frame was not observed");
  if (
    !evidence.mouseEvents.some(
      (event) =>
        event.isTrusted &&
        event.locked &&
        Math.hypot(event.movementX, event.movementY) >= MIN_TRUSTED_MOUSE_DELTA,
    )
  ) {
    errors.push("trusted locked mouse look was not observed");
  }
  const firstDirection = evidence.outgoingDirections[0];
  if (
    firstDirection === undefined ||
    !evidence.outgoingDirections.some(
      (direction) =>
        vectorDistance(firstDirection, direction) >= MIN_LOOK_DIRECTION_DELTA,
    )
  ) {
    errors.push("outgoing look direction did not change");
  }
  if (
    Math.max(0, ...evidence.authoritativeDisplacements) <
    MIN_AUTHORITATIVE_DISPLACEMENT
  ) {
    errors.push("authoritative peer position did not move one block");
  }
  return errors;
}

export function movementDifferenceErrors(
  difference: CanvasDifference,
): string[] {
  const errors: string[] = [];
  if (difference.changedRatio < MIN_MOVEMENT_CHANGED_RATIO) {
    errors.push("changed pixel ratio is too low");
  }
  if (difference.meanChannelDelta < MIN_MOVEMENT_MEAN_CHANNEL_DELTA) {
    errors.push("mean channel delta is too low");
  }
  return errors;
}

function vectorDistance(
  left: readonly number[],
  right: readonly number[],
): number {
  return Math.hypot(
    (left[0] ?? 0) - (right[0] ?? 0),
    (left[1] ?? 0) - (right[1] ?? 0),
    (left[2] ?? 0) - (right[2] ?? 0),
  );
}
