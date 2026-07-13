import type { Inputs, RigidControls, VoxelInteract } from "@voxelize/core";
import { Raycaster, Vector2, Vector3, type Object3D } from "three";

import type { MovementInput } from "./network";

export interface WorldInputActions {
  attack(): void;
  dropSlot(slot: number): void;
  mining(
    action: "cancel" | "maintain" | "start",
    voxel?: [number, number, number],
  ): void;
  movement(input: MovementInput): void;
}

const MAINTAIN_INTERVAL_MS = 125;
const MOVEMENT_INTERVAL_MS = 50;
const MELEE_REACH = 3;
const CROSSHAIR = new Vector2(0, 0);
const VOXEL_HALF_DIAGONAL = Math.sqrt(3) / 2;

export class WorldInputController {
  private readonly direction = new Vector3();
  private readonly raycaster = new Raycaster();
  private miningTarget: [number, number, number] | null = null;
  private nextMaintainAt = 0;
  private selectedSlot = 0;
  private lastMovementAt = 0;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly controls: RigidControls,
    private readonly inputs: Inputs<"disabled" | "in-game">,
    private readonly interact: VoxelInteract,
    private readonly peerTargets: Object3D,
    private readonly actions: WorldInputActions,
  ) {
    canvas.addEventListener("mousedown", this.handleMouseDown);
    canvas.addEventListener("contextmenu", this.preventContextMenu);
    document.addEventListener("mouseup", this.handleMouseUp);
    document.addEventListener("keydown", this.handleKeyDown);
    document.addEventListener("visibilitychange", this.handleVisibility);
    controls.on("lock", this.handleLock);
    controls.on("unlock", this.handleUnlock);
    inputs.setNamespace("disabled");
  }

  update(now = performance.now()): void {
    this.updateMining(now);
    this.sendMovement(now);
  }

  dispose(): void {
    this.cancelMining();
    this.actions.movement(stoppedMovement(this.controls));
    if (document.pointerLockElement === this.canvas) {
      document.exitPointerLock();
    }
    this.canvas.removeEventListener("mousedown", this.handleMouseDown);
    this.canvas.removeEventListener("contextmenu", this.preventContextMenu);
    document.removeEventListener("mouseup", this.handleMouseUp);
    document.removeEventListener("keydown", this.handleKeyDown);
    document.removeEventListener("visibilitychange", this.handleVisibility);
    this.controls.off("lock", this.handleLock);
    this.controls.off("unlock", this.handleUnlock);
  }

  private handleMouseDown = (event: MouseEvent): void => {
    if (!this.controls.isLocked) return;
    if (event.button === 2) {
      this.safe(() => this.actions.attack());
      return;
    }
    if (event.button !== 0) return;
    const voxel = this.interact.target;
    if (this.peerHitPrecedes(voxel)) {
      this.safe(() => this.actions.attack());
    } else if (voxel !== null) {
      this.startMining(voxel);
    } else {
      this.safe(() => this.actions.attack());
    }
  };

  private handleMouseUp = (event: MouseEvent): void => {
    if (event.button === 0) this.cancelMining();
  };

  private handleKeyDown = (event: KeyboardEvent): void => {
    if (!this.controls.isLocked || event.repeat) return;
    const slot = slotForCode(event.code);
    if (slot !== null) this.selectedSlot = slot;
    if (event.code === "KeyQ") {
      this.safe(() => this.actions.dropSlot(this.selectedSlot));
    }
  };

  private handleLock = (): void => this.inputs.setNamespace("in-game");

  private handleUnlock = (): void => {
    this.inputs.setNamespace("disabled");
    this.controls.resetMovements();
    this.cancelMining();
    this.sendMovement(performance.now(), true);
  };

  private handleVisibility = (): void => {
    if (document.hidden) this.handleUnlock();
  };

  private preventContextMenu = (event: Event): void => event.preventDefault();

  private startMining(voxel: [number, number, number]): void {
    this.miningTarget = [...voxel];
    this.nextMaintainAt = performance.now() + MAINTAIN_INTERVAL_MS;
    this.safe(() =>
      this.actions.mining("start", this.miningTarget ?? undefined),
    );
  }

  private updateMining(now: number): void {
    if (this.miningTarget === null) return;
    const current = this.interact.target;
    if (current === null) {
      this.cancelMining();
      return;
    }
    if (!sameVoxel(current, this.miningTarget)) {
      this.startMining(current);
      return;
    }
    if (now >= this.nextMaintainAt) {
      this.nextMaintainAt = now + MAINTAIN_INTERVAL_MS;
      this.safe(() => this.actions.mining("maintain"));
    }
  }

  private cancelMining(): void {
    if (this.miningTarget === null) return;
    this.miningTarget = null;
    this.safe(() => this.actions.mining("cancel"));
  }

  private sendMovement(now: number, force = false): void {
    const input = movementInput(this.controls, this.direction);
    if (!isMovementDue(now, this.lastMovementAt, force)) return;
    this.lastMovementAt = now;
    this.actions.movement(input);
  }

  private peerHitPrecedes(voxel: [number, number, number] | null): boolean {
    this.raycaster.setFromCamera(CROSSHAIR, this.controls.camera);
    const hit = this.raycaster
      .intersectObject(this.peerTargets, true)
      .find((candidate) => candidate.distance <= MELEE_REACH);
    return (
      hit !== undefined &&
      isPeerHitCloser(hit.distance, this.raycaster.ray.origin, voxel)
    );
  }

  private safe(action: () => void): void {
    try {
      action();
    } catch {
      this.handleUnlock();
    }
  }
}

function movementInput(
  controls: RigidControls,
  direction: Vector3,
): MovementInput {
  controls.camera.getWorldDirection(direction).normalize();
  let forward = controls.movements.front ? 1 : controls.movements.back ? -1 : 0;
  let right = controls.movements.right ? 1 : controls.movements.left ? -1 : 0;
  const length = Math.hypot(forward, right);
  if (length > 1) {
    forward /= length;
    right /= length;
  }
  return {
    direction: [direction.x, direction.y, direction.z],
    movement: { forward, right, jump: controls.movements.up },
  };
}

function stoppedMovement(controls: RigidControls): MovementInput {
  const direction = new Vector3();
  controls.camera.getWorldDirection(direction).normalize();
  return {
    direction: [direction.x, direction.y, direction.z],
    movement: { forward: 0, right: 0, jump: false },
  };
}

function sameVoxel(
  left: [number, number, number],
  right: [number, number, number],
): boolean {
  return left.every((value, index) => value === right[index]);
}

function slotForCode(code: string): number | null {
  if (/^Digit[1-9]$/.test(code)) return Number(code.at(-1)) - 1;
  if (code === "Digit0") return 9;
  if (code === "Minus") return 10;
  if (code === "Equal") return 11;
  return null;
}

export function isPeerHitCloser(
  peerDistance: number,
  rayOrigin: Vector3,
  voxel: [number, number, number] | null,
): boolean {
  if (voxel === null) return true;
  const voxelDistance = Math.max(
    0,
    rayOrigin.distanceTo(
      new Vector3(voxel[0] + 0.5, voxel[1] + 0.5, voxel[2] + 0.5),
    ) - VOXEL_HALF_DIAGONAL,
  );
  return peerDistance < voxelDistance;
}

export function isMovementDue(
  now: number,
  lastSentAt: number,
  force = false,
): boolean {
  return force || now - lastSentAt >= MOVEMENT_INTERVAL_MS;
}
