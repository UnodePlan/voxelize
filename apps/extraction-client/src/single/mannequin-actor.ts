/**
 * 假人统一接口：兼容 Voxelize Character 与其它人形实现。
 */

import type { Character } from "@voxelize/core";
import type { Object3D } from "three";

import { disposeObjectTree } from "../game/object-disposal";

export interface MannequinActor {
  readonly eyeHeight: number;
  set(position: number[], direction: number[]): void;
  update(): void;
  playArmSwingAnimation(): void;
  snapToTarget(): void;
  readonly root: Object3D;
  dispose(): void;
  /** 第三人称右臂手持物；可选（MC biped 实现，Voxelize Character 可无） */
  setHeldItem?(item: Object3D | null): void;
  /**
   * 受击击退冲量（世界坐标；mass≈1 时等同初速度）。
   * 可选：无实现则忽略击退。
   */
  applyKnockback?(impulse: readonly [number, number, number]): void;
  /** 仍在击退位移中（demo 应暂停巡逻 set） */
  isKnockedBack?(): boolean;
  /** 清零击退速度（击倒 / 复活） */
  clearKnockback?(): void;
  /** 击退中更新贴地眼高（随地形） */
  setKnockbackGroundEyeY?(eyeY: number): void;
}

/** 把 Voxelize Character 包成 MannequinActor */
export function wrapCharacterMannequin(character: Character): MannequinActor {
  return {
    get eyeHeight() {
      return character.eyeHeight;
    },
    set(position, direction) {
      character.set(position, direction);
    },
    update() {
      character.update();
    },
    playArmSwingAnimation() {
      character.playArmSwingAnimation();
    },
    snapToTarget() {
      character.snapToTarget();
    },
    root: character,
    dispose() {
      disposeObjectTree(character, true);
    },
  };
}
