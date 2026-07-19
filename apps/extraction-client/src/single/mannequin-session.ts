/**
 * 出生点假人近战 / 击倒 / 复活节奏。
 * 从 SinglePlayerController 拆出。
 */

import {
  applyDamage,
  attackDamageHalfHearts,
  fullHealth,
  isDead,
  knockbackImpulse,
  LOCAL_ATTACK_COOLDOWN_MS,
  LOCAL_ATTACK_RANGE,
} from "./combat";
import type { LocalWorldRuntime } from "./runtime";
import { heldToolFromSlot } from "./viewmodel";

export interface MannequinSessionState {
  health: number;
  respawnAt: number;
  deathXZ: [number, number] | null;
  attackCooldownUntil: number;
}

export interface MannequinAttackInput {
  pointerLocked: boolean;
  primaryHeld: boolean;
  wasPrimaryHeld: boolean;
  selectedSlot: number;
  now: number;
  state: MannequinSessionState;
}

export type MannequinAttackResult =
  | { kind: "miss" }
  | { kind: "aiming" }
  | {
      kind: "hit";
      state: MannequinSessionState;
      targetName: string | null;
      notice: string | null;
      killDrop: {
        tool: "sword" | "pickaxe";
        position: [number, number, number];
      } | null;
      sfxRate: number;
    };

export function tryMannequinAttack(
  runtime: LocalWorldRuntime,
  input: MannequinAttackInput,
): MannequinAttackResult {
  if (!input.pointerLocked || !input.primaryHeld) return { kind: "miss" };
  if (input.state.health <= 0) return { kind: "miss" };

  const hitDist = runtime.raycastMannequin(LOCAL_ATTACK_RANGE);
  if (hitDist === null) return { kind: "miss" };

  const rising = !input.wasPrimaryHeld;
  const cooled = input.now >= input.state.attackCooldownUntil;
  if (!rising && !cooled) return { kind: "aiming" };
  if (!cooled) return { kind: "aiming" };

  const tool = heldToolFromSlot(input.selectedSlot);
  const damage = attackDamageHalfHearts(tool);
  let health = applyDamage(input.state.health, damage);
  const attackCooldownUntil = input.now + LOCAL_ATTACK_COOLDOWN_MS;
  runtime.playAttackSwing();
  runtime.playMannequinHurtFeedback();
  const dir = runtime.getDirection();
  runtime.applyMannequinKnockback(
    knockbackImpulse(tool, [dir.x, dir.y, dir.z]),
  );

  if (isDead(health)) {
    const kill = finishMannequinKill(runtime, input.now);
    return {
      kind: "hit",
      state: {
        health: 0,
        respawnAt: kill.respawnAt,
        deathXZ: kill.deathXZ,
        attackCooldownUntil,
      },
      targetName: null,
      notice: kill.notice,
      killDrop: kill.drop,
      sfxRate: tool === "sword" ? 1.15 : 1.05,
    };
  }

  const heartsLeft = (health / 2).toFixed(health % 2 === 0 ? 0 : 1);
  return {
    kind: "hit",
    state: {
      health,
      respawnAt: input.state.respawnAt,
      deathXZ: input.state.deathXZ,
      attackCooldownUntil,
    },
    targetName: `Steve · ${heartsLeft}♥`,
    notice: null,
    killDrop: null,
    sfxRate: tool === "sword" ? 1.15 : 1.05,
  };
}

function finishMannequinKill(
  runtime: LocalWorldRuntime,
  now: number,
): {
  deathXZ: [number, number];
  respawnAt: number;
  notice: string;
  drop: {
    tool: "sword" | "pickaxe";
    position: [number, number, number];
  } | null;
} {
  const eye = runtime.getMannequinEyePosition();
  const deathXZ: [number, number] = eye !== null ? [eye[0], eye[2]] : [0, 0];
  const dropPos: [number, number, number] =
    eye !== null
      ? [eye[0], eye[1] - 1.2, eye[2]]
      : [deathXZ[0], 1, deathXZ[1]];
  const held = runtime.killMannequin();
  return {
    deathXZ,
    respawnAt: now + 3_000,
    notice:
      held !== null ? "击倒了 Steve · 掉落了物品" : "击倒了 Steve",
    drop:
      held !== null
        ? { tool: held, position: dropPos }
        : null,
  };
}

export function tickMannequinRespawn(
  runtime: LocalWorldRuntime | null,
  state: MannequinSessionState,
  now: number,
): {
  state: MannequinSessionState;
  notice: string | null;
} {
  if (state.health > 0) return { state, notice: null };
  if (state.respawnAt <= 0 || now < state.respawnAt) {
    return { state, notice: null };
  }
  const near = state.deathXZ ?? [0, 0];
  runtime?.respawnMannequinNear(near);
  return {
    state: {
      health: fullHealth(),
      respawnAt: 0,
      deathXZ: null,
      attackCooldownUntil: state.attackCooldownUntil,
    },
    notice: "Steve 在附近重新站了起来",
  };
}
