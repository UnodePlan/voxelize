/**
 * 单机场景中的 MC 人形：出生点假人巡逻 + 第一人称自身身体。
 * 从 runtime 抽出，避免 runtime 继续膨胀。
 */

import type { Mesh, Object3D, Vector3 } from "three";

import type { MannequinActor } from "./mannequin-actor";
import { LOCAL_SPAWN_XZ } from "./map-layout";
import {
  createMcBipedMannequin,
  type McBipedMannequin,
} from "./mc-biped";
import {
  createMcHeldItemMesh,
  disposeMcHeldItemMesh,
} from "./mc-held-item";
import { McMannequinDemo } from "./mc-mannequin-demo";

export interface McSceneActors {
  mannequin: MannequinActor | null;
  mannequinDemo: McMannequinDemo | null;
  selfBody: McBipedMannequin | null;
  /** 假人持有的手持 mesh，dispose 时释放 */
  heldMeshes: Mesh[];
}

export function createEmptyMcSceneActors(): McSceneActors {
  return {
    mannequin: null,
    mannequinDemo: null,
    selfBody: null,
    heldMeshes: [],
  };
}

export interface SpawnMcSceneOptions {
  isDisposed: () => boolean;
  addToWorld: (root: Object3D) => void;
  surfaceY: (x: number, z: number) => number;
  /** RigidControls 眼高（脚底 → 眼），用于缩放自身身体 */
  eyeFromFeet: number;
}

/**
 * 异步生成 Steve 假人（带剑/镐）与第一人称自身身体。
 * 调用方持有返回的 actors 引用并在 dispose 时调用 disposeMcSceneActors。
 */
export async function spawnMcSceneActors(
  options: SpawnMcSceneOptions,
): Promise<McSceneActors> {
  const actors = createEmptyMcSceneActors();
  await Promise.all([
    spawnSteveMannequin(actors, options),
    spawnSelfBody(actors, options),
  ]);
  return actors;
}

export function updateMcSceneActors(
  actors: McSceneActors,
  deltaMs: number,
  options: {
    /** 世界就绪后才驱动假人巡逻 */
    runDemo: boolean;
    selfFrame: {
      position: Vector3;
      lookDir: Vector3;
      moving: boolean;
    } | null;
  },
): void {
  if (options.runDemo) {
    actors.mannequinDemo?.update(deltaMs);
  }
  if (options.selfFrame !== null && actors.selfBody !== null) {
    actors.selfBody.syncEyeFrame(
      options.selfFrame.position,
      options.selfFrame.lookDir,
      options.selfFrame.moving,
    );
    actors.selfBody.update();
  }
}

export function disposeMcSceneActors(
  actors: McSceneActors,
  removeFromWorld: (root: Object3D) => void,
): void {
  if (actors.mannequin !== null) {
    removeFromWorld(actors.mannequin.root);
    actors.mannequin.dispose();
    actors.mannequin = null;
  }
  actors.mannequinDemo = null;
  if (actors.selfBody !== null) {
    removeFromWorld(actors.selfBody.root);
    actors.selfBody.dispose();
    actors.selfBody = null;
  }
  for (const mesh of actors.heldMeshes) {
    disposeMcHeldItemMesh(mesh);
  }
  actors.heldMeshes = [];
}

async function spawnSteveMannequin(
  actors: McSceneActors,
  options: SpawnMcSceneOptions,
): Promise<void> {
  try {
    const actor = await createMcBipedMannequin({ username: "Steve" });
    if (options.isDisposed()) {
      actor.dispose();
      return;
    }

    const [sword, pickaxe] = await Promise.all([
      createMcHeldItemMesh("sword"),
      createMcHeldItemMesh("pickaxe"),
    ]);
    if (options.isDisposed()) {
      actor.dispose();
      disposeMcHeldItemMesh(sword);
      disposeMcHeldItemMesh(pickaxe);
      return;
    }

    actors.heldMeshes.push(sword, pickaxe);
    actors.mannequin = actor;
    options.addToWorld(actor.root);

    const fromX = LOCAL_SPAWN_XZ[0] + 2.5;
    const toX = LOCAL_SPAWN_XZ[0] + 7.5;
    const z = LOCAL_SPAWN_XZ[1] + 0.5;
    actors.mannequinDemo = new McMannequinDemo({
      character: actor,
      surfaceY: options.surfaceY,
      fromXZ: [fromX, z],
      toXZ: [toX, z],
      walkSpeed: 2.4,
      walkLegSeconds: 5,
      mineSeconds: 3.2,
      digIntervalSeconds: 0.4,
      heldItems: { walk: sword, mine: pickaxe },
    });
  } catch (error) {
    console.error("Steve 假人加载失败", error);
  }
}

async function spawnSelfBody(
  actors: McSceneActors,
  options: SpawnMcSceneOptions,
): Promise<void> {
  try {
    const body = await createMcBipedMannequin({ username: "You" });
    if (options.isDisposed()) {
      body.dispose();
      return;
    }
    body.enableFirstPersonSelfView();
    // controls 眼高对齐 MC 1.75 模型，避免腿埋地或悬空
    const scale = options.eyeFromFeet / Math.max(1e-6, body.eyeHeight);
    body.root.scale.setScalar(scale);
    actors.selfBody = body;
    options.addToWorld(body.root);
  } catch (error) {
    console.error("第一人称自身身体加载失败", error);
  }
}
