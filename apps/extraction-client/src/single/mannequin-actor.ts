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
