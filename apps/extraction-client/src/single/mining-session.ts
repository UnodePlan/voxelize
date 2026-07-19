/**
 * 单机挖掘会话：按住挖掘 / 裂纹同步 / 完成 claim。
 * 从 SinglePlayerController 拆出，控制 controller 体量。
 */

import type { LocalBlockDurability } from "./block-durability";
import {
  getBlockMiningProfile,
  LOCAL_BLOCK_DEBRIS_COLORS,
  LOCAL_BLOCK_IDS,
} from "./blocks";
import {
  digIntervalMs,
  digRateFor,
  harvestDrop,
  miningDurationMs,
} from "./mining";
import type { LocalRuntimeFrame, LocalWorldRuntime } from "./runtime";
import type { LocalGameAction, LocalResourceKey } from "./state";
import { voxelKey } from "./state";
import { heldToolFromSlot, type LocalHeldTool } from "./viewmodel";

export interface MiningSessionDeps {
  selectedSlot: number;
  mining: {
    targetKey: string;
    requiredMs: number;
  } | null;
  claimed: Set<string>;
  durability: LocalBlockDurability;
  primaryHeld: boolean;
  pointerLocked: boolean;
  lastDigAt: number;
  dispatch(action: LocalGameAction): void;
  stopActiveMining(): void;
  playSfx(
    name: "dig" | "break" | "drop",
    opts?: { rate?: number },
  ): void;
  dropLoot(
    resource: LocalResourceKey,
    quantity: number,
    position: [number, number, number],
    now: number,
    excludeMs: number,
  ): void;
}

export function advanceMiningSession(
  deps: MiningSessionDeps,
  runtime: LocalWorldRuntime,
  frame: LocalRuntimeFrame,
  now: number,
): number {
  const tool = heldToolFromSlot(deps.selectedSlot);
  const target = frame.target;
  const profile = target === null ? null : getBlockMiningProfile(target.id);
  const key = target === null ? null : voxelKey(target.voxel);
  if (
    !deps.primaryHeld ||
    !deps.pointerLocked ||
    target === null ||
    profile === null ||
    key === null ||
    deps.claimed.has(key)
  ) {
    deps.stopActiveMining();
    return deps.lastDigAt;
  }

  const requiredMs = miningDurationMs(profile, tool);
  const drop = harvestDrop(profile.drop, tool, profile);
  const priorDamage = deps.durability.get(key, target.id);
  if (
    deps.mining?.targetKey !== key ||
    deps.mining.requiredMs !== requiredMs
  ) {
    deps.dispatch({
      type: "MINING_STARTED",
      target: target.voxel,
      resource: drop,
      displayName: profile.displayName,
      requiredMs,
      elapsedMs: priorDamage * requiredMs,
    });
    deps.playSfx("dig", { rate: digRateFor(drop, tool) });
    deps.lastDigAt = now;
  }

  const amount = requiredMs > 0 ? Math.max(0, frame.deltaMs) / requiredMs : 1;
  const damage = deps.durability.apply(key, target.id, amount);
  deps.dispatch({
    type: "MINING_ADVANCED",
    deltaMs: frame.deltaMs,
    targetKey: key,
    elapsedMs: damage * requiredMs,
  });
  let lastDigAt = deps.lastDigAt;
  if (now - lastDigAt > digIntervalMs(tool)) {
    deps.playSfx("dig", { rate: digRateFor(drop, tool) });
    lastDigAt = now;
  }
  if (damage >= 1) {
    completeMiningSession(deps, runtime, target, drop, now, tool);
  }
  return lastDigAt;
}

export function syncBreakCrackEntries(
  deps: Pick<MiningSessionDeps, "claimed" | "durability">,
  runtime: LocalWorldRuntime,
): void {
  const entries = deps.durability.snapshot().flatMap((snap) => {
    if (deps.claimed.has(snap.key) || !(snap.damage > 0)) return [];
    if (runtime.world.getVoxelAt(...snap.voxel) !== snap.blockId) {
      return [];
    }
    return [
      {
        key: snap.key,
        progress: snap.damage,
        voxel: snap.voxel,
      },
    ];
  });
  runtime.setBreakCracks(entries);
}

function completeMiningSession(
  deps: MiningSessionDeps,
  runtime: LocalWorldRuntime,
  target: { id: number; voxel: [number, number, number] },
  drop: LocalResourceKey | null,
  now: number,
  tool: LocalHeldTool,
): void {
  const key = voxelKey(target.voxel);
  if (
    deps.claimed.has(key) ||
    runtime.world.getVoxelAt(...target.voxel) !== target.id
  ) {
    deps.durability.clear(key);
    deps.stopActiveMining();
    return;
  }
  deps.claimed.add(key);
  deps.durability.clear(key);
  try {
    runtime.adapter.applyServerVoxelUpdate(
      runtime.world,
      target.voxel,
      LOCAL_BLOCK_IDS.air,
    );
  } catch {
    deps.claimed.delete(key);
    deps.stopActiveMining();
    return;
  }
  runtime.playBlockBreakBurst(
    target.voxel,
    LOCAL_BLOCK_DEBRIS_COLORS[target.id] ?? "#888888",
    now,
  );
  deps.playSfx("break", { rate: digRateFor(drop, tool) });
  if (drop !== null) {
    deps.dropLoot(
      drop,
      1,
      [target.voxel[0] + 0.5, target.voxel[1] + 0.55, target.voxel[2] + 0.5],
      now,
      280,
    );
    deps.playSfx("drop");
  }
  deps.stopActiveMining();
}
