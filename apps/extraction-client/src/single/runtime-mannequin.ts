/**
 * Runtime 侧假人查询与击倒/复活。
 * 从 LocalWorldRuntime 拆出，控制 runtime 体量。
 */

import type { RigidControls } from "@voxelize/core";
import type { Vector3 } from "three";

import { humanoidAabb, raycastAabb } from "./combat";
import type { McSceneActors } from "./mc-scene-actors";

export function raycastMannequinActor(
  actors: McSceneActors,
  eye: Vector3,
  direction: Vector3,
  maxDistance: number,
): number | null {
  const actor = actors.mannequin;
  const demo = actors.mannequinDemo;
  if (actor === null || !actor.root.visible) return null;
  if (demo !== null && !demo.isAlive) return null;
  const pos = actor.root.position;
  const aabb = humanoidAabb([pos.x, pos.y, pos.z], actor.eyeHeight);
  return raycastAabb(
    [eye.x, eye.y, eye.z],
    [direction.x, direction.y, direction.z],
    aabb,
    maxDistance,
  );
}

export function playMannequinHurt(actors: McSceneActors): void {
  actors.mannequin?.playArmSwingAnimation();
}

export function applyMannequinKnockbackImpulse(
  actors: McSceneActors,
  impulse: readonly [number, number, number],
): boolean {
  const actor = actors.mannequin;
  const demo = actors.mannequinDemo;
  if (actor === null || actor.applyKnockback === undefined) return false;
  if (demo !== null && !demo.isAlive) return false;
  actor.applyKnockback(impulse);
  return true;
}

export function killMannequinActor(
  actors: McSceneActors,
): "sword" | "pickaxe" | null {
  const demo = actors.mannequinDemo;
  if (demo === null) {
    const actor = actors.mannequin;
    if (actor !== null) {
      actor.root.visible = false;
      actor.setHeldItem?.(null);
      actor.clearKnockback?.();
    }
    return null;
  }
  return demo.kill();
}

export function mannequinEyePosition(
  actors: McSceneActors,
): [number, number, number] | null {
  const demo = actors.mannequinDemo;
  if (demo !== null) return demo.getEyePosition();
  const actor = actors.mannequin;
  if (actor === null) return null;
  const p = actor.root.position;
  return [p.x, p.y, p.z];
}

export function respawnMannequinNearPoint(
  actors: McSceneActors,
  near: readonly [number, number],
  pathHalfLength = 2.5,
): void {
  const demo = actors.mannequinDemo;
  if (demo === null) {
    const actor = actors.mannequin;
    if (actor !== null) actor.root.visible = true;
    return;
  }
  const angle = Math.random() * Math.PI * 2;
  const dist = 2 + Math.random() * 2;
  const x = near[0] + Math.cos(angle) * dist;
  const z = near[1] + Math.sin(angle) * dist;
  demo.respawnNear(x, z, pathHalfLength);
}

export function applyPlayerKnockbackImpulse(
  controls: RigidControls,
  impulse: readonly [number, number, number],
): void {
  controls.body.applyImpulse([impulse[0], impulse[1], impulse[2]]);
}
