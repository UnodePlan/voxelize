import type {
  GameplayStateData,
  ResourceKey,
} from "../../../../contracts/extraction/v1/typescript";

export type LiveVector3 = [number, number, number];

export function readRecord(
  value: unknown,
  label: string,
): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    throw new Error(`${label} must be an object`);
  }
  return value as Record<string, unknown>;
}

export function readVector3(value: unknown, label: string): LiveVector3 {
  if (
    !Array.isArray(value) ||
    value.length !== 3 ||
    !value.every((part) => typeof part === "number" && Number.isFinite(part))
  ) {
    throw new Error(`${label} must be a finite three-component vector`);
  }
  return [value[0] as number, value[1] as number, value[2] as number];
}

export function directionBetween(
  from: LiveVector3,
  to: LiveVector3,
): LiveVector3 {
  const delta: LiveVector3 = [
    to[0] - from[0],
    to[1] - from[1],
    to[2] - from[2],
  ];
  const length = Math.hypot(...delta);
  if (!Number.isFinite(length) || length <= Number.EPSILON) {
    throw new Error("cannot derive direction from coincident positions");
  }
  return delta.map((part) => part / length) as LiveVector3;
}

export function horizontalDistance(
  left: LiveVector3,
  right: LiveVector3,
): number {
  return Math.hypot(left[0] - right[0], left[2] - right[2]);
}

export function surfaceVoxelBelow(position: LiveVector3): LiveVector3 {
  return [Math.floor(position[0]), 48, Math.floor(position[2])];
}

export function supportedSurfaceVoxelTowardCenter(
  position: LiveVector3,
): LiveVector3 {
  const target = surfaceVoxelBelow(position);
  // 沿离中心更远的主轴内移两格，保留脚下支撑；射线会从中间地表上方越过。
  if (Math.abs(position[0]) >= Math.abs(position[2])) {
    target[0] += position[0] >= 0 ? -2 : 2;
  } else {
    target[2] += position[2] >= 0 ? -2 : 2;
  }
  return target;
}

export function voxelCenter(voxel: LiveVector3): LiveVector3 {
  return [voxel[0] + 0.5, voxel[1] + 0.5, voxel[2] + 0.5];
}

export function visibleTopFacePoint(voxel: LiveVector3): LiveVector3 {
  // 略低于顶面，确保权威体素射线进入目标，而不是停在边界或先碰近处地表。
  return [voxel[0] + 0.5, voxel[1] + 0.999, voxel[2] + 0.5];
}

export function inventoryResourceCount(
  state: GameplayStateData,
  resource: ResourceKey,
): number {
  return state.inventory.slots.reduce(
    (total, slot) => total + (slot?.resource === resource ? slot.quantity : 0),
    0,
  );
}

export function acceptedGameplaySequence(state: GameplayStateData): number {
  return Math.max(
    state.attack.acceptedSequence ?? 0,
    state.mining.data.acceptedSequence ?? 0,
    state.inventory.lastDropSequence ?? 0,
  );
}
