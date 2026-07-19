/**
 * 自身第三人称手持 mesh 缓存与挂载。
 * 从 LocalWorldRuntime 拆出，避免 runtime 继续膨胀。
 */

import type { Mesh } from "three";

import {
  createMcHeldContentMesh,
  disposeMcHeldItemMesh,
} from "./mc-held-item";
import {
  heldContentKey,
  type LocalHeldContent,
} from "./held-content";

/** 右臂可挂手持物的最小接口（mc-biped / 假人） */
export interface HeldItemBody {
  setHeldItem(mesh: Mesh | null): void;
  setHoldingItem(holding: boolean): void;
}

export class SelfHeldVisual {
  private readonly meshes = new Map<string, Mesh>();
  private key: string | null = null;
  private loadGen = 0;
  private disposed = false;

  /** 强制下次 sync 重新挂载（身体异步就绪后） */
  invalidateAttachment(): void {
    this.key = null;
  }

  /**
   * 按 content 同步到 body.heldSlot。
   * getLiveContent：异步返回后校验是否仍是当前握持。
   */
  async sync(
    content: LocalHeldContent,
    body: HeldItemBody | null,
    getLiveContent: () => LocalHeldContent,
  ): Promise<void> {
    if (this.disposed || body === null) return;

    if (content.kind === "empty") {
      this.key = null;
      body.setHeldItem(null);
      return;
    }

    const key = heldContentKey(content);
    if (key === this.key) {
      body.setHoldingItem(true);
      return;
    }

    let mesh = this.meshes.get(key);
    if (mesh === undefined) {
      const gen = ++this.loadGen;
      try {
        const created = await createMcHeldContentMesh(content);
        if (this.disposed || gen !== this.loadGen) {
          if (created !== null) disposeMcHeldItemMesh(created);
          return;
        }
        if (created === null) {
          body.setHeldItem(null);
          this.key = null;
          return;
        }
        this.meshes.set(key, created);
        mesh = created;
      } catch (error) {
        console.warn("[single] 第三人称手持加载失败", key, error);
        return;
      }
    }

    if (heldContentKey(getLiveContent()) !== key) return;
    this.key = key;
    body.setHeldItem(mesh);
  }

  dispose(): void {
    this.disposed = true;
    for (const mesh of this.meshes.values()) {
      disposeMcHeldItemMesh(mesh);
    }
    this.meshes.clear();
    this.key = null;
  }
}
