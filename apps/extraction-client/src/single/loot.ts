import {
  BoxGeometry,
  DoubleSide,
  Group,
  Mesh,
  MeshBasicMaterial,
  NearestFilter,
  PlaneGeometry,
  SRGBColorSpace,
  Texture,
  Vector3,
  type Object3D,
} from "three";

import diamondOreUrl from "../assets/single/blocks/diamond_ore.png";
import dirtUrl from "../assets/single/blocks/dirt.png";
import goldOreUrl from "../assets/single/blocks/gold_ore.png";
import grassTopUrl from "../assets/single/blocks/grass_top.png";
import planksUrl from "../assets/single/blocks/oak_planks.png";
import stoneUrl from "../assets/single/blocks/stone.png";
import ironPickaxeUrl from "../assets/single/items/iron_pickaxe.png";
import ironSwordUrl from "../assets/single/items/iron_sword.png";

import { disposeObjectTree } from "../game/object-disposal";

import type { McHeldTool } from "./mc-held-item";
import {
  LOCAL_RESOURCE_KEYS,
  type LocalResourceKey,
} from "./state";

/** 拾取回调载荷：资源进背包 / 工具仅通知 */
export type LocalLootPickup =
  | { kind: "resource"; resource: LocalResourceKey; quantity: number }
  | { kind: "tool"; tool: McHeldTool };

interface LocalLootEntry {
  availableAt: number;
  baseY: number;
  mesh: Mesh;
  /** 资源数量；工具恒为 1 */
  quantity: number;
  pickup: LocalLootPickup;
  /** 弹出初速后的竖直速度，落地后归零并改为悬浮 */
  vy: number;
  settled: boolean;
  /** 工具用独立 plane 几何，dispose 时释放 */
  ownsGeometry: boolean;
}

/** 树叶无独立 16px 图时复用草顶作为背包/掉落标识 */
const RESOURCE_TEXTURE_URL: Record<LocalResourceKey, string> = {
  dirt: dirtUrl,
  grass: grassTopUrl,
  stone: stoneUrl,
  planks: planksUrl,
  leaves: grassTopUrl,
  gold: goldOreUrl,
  diamond: diamondOreUrl,
};

const RESOURCE_FALLBACK_COLOR: Record<LocalResourceKey, string> = {
  dirt: "#79523d",
  grass: "#5d8a3a",
  stone: "#8a8a8a",
  planks: "#9a7348",
  leaves: "#3d8c3a",
  gold: "#e0b43b",
  diamond: "#3dd2cc",
};

const TOOL_TEXTURE_URL: Record<McHeldTool, string> = {
  sword: ironSwordUrl,
  pickaxe: ironPickaxeUrl,
};

const TOOL_FALLBACK_COLOR: Record<McHeldTool, string> = {
  sword: "#c0c0c0",
  pickaxe: "#a8a8a8",
};

/** 原版感掉落：迷你方块/道具实体 + 弹出/悬浮 + 走近拾取 */
export class LocalLootSystem {
  private readonly root = new Group();
  private readonly entries: LocalLootEntry[] = [];
  private readonly textureCache = new Map<string, Texture | null>();
  private readonly sharedGeometry = new BoxGeometry(0.36, 0.36, 0.36);
  private disposed = false;

  constructor(world: Object3D) {
    this.root.name = "single-local-loot";
    world.add(this.root);
    // 预载全部资源贴图
    for (const key of LOCAL_RESOURCE_KEYS) {
      void this.ensureTexture(`res:${key}`, RESOURCE_TEXTURE_URL[key]);
    }
    void this.ensureTexture("tool:sword", TOOL_TEXTURE_URL.sword);
    void this.ensureTexture("tool:pickaxe", TOOL_TEXTURE_URL.pickaxe);
  }

  drop(
    resource: LocalResourceKey,
    quantity: number,
    position: readonly [number, number, number],
    now: number,
    pickupDelayMs: number,
  ): void {
    if (this.disposed || quantity <= 0) return;
    const cacheKey = `res:${resource}`;
    const material = new MeshBasicMaterial({
      color: RESOURCE_FALLBACK_COLOR[resource],
      map: this.textureCache.get(cacheKey) ?? null,
      transparent: true,
      alphaTest: 0.05,
      side: DoubleSide,
      toneMapped: false,
    });
    const mesh = new Mesh(this.sharedGeometry, material);
    mesh.position.set(position[0], position[1], position[2]);
    mesh.rotation.set(0.12, Math.random() * Math.PI * 2, 0.08);
    mesh.userData.resource = resource;
    this.root.add(mesh);
    this.entries.push({
      availableAt: now + pickupDelayMs,
      baseY: position[1],
      mesh,
      quantity,
      pickup: { kind: "resource", resource, quantity },
      vy: 2.4 + Math.random() * 0.8,
      settled: false,
      ownsGeometry: false,
    });
    void this.ensureTexture(cacheKey, RESOURCE_TEXTURE_URL[resource]).then(
      (texture) => {
        if (this.disposed || texture === null) return;
        if (material.map !== texture) {
          material.map = texture;
          material.color.set("#ffffff");
          material.needsUpdate = true;
        }
      },
    );
  }

  /**
   * 掉落手持工具（平面图标）；拾取不进资源背包，由 accept 处理。
   */
  dropTool(
    tool: McHeldTool,
    position: readonly [number, number, number],
    now: number,
    pickupDelayMs: number,
  ): void {
    if (this.disposed) return;
    const cacheKey = `tool:${tool}`;
    const material = new MeshBasicMaterial({
      color: TOOL_FALLBACK_COLOR[tool],
      map: this.textureCache.get(cacheKey) ?? null,
      transparent: true,
      alphaTest: 0.12,
      side: DoubleSide,
      toneMapped: false,
      depthWrite: false,
    });
    const geometry = new PlaneGeometry(0.55, 0.55);
    const mesh = new Mesh(geometry, material);
    mesh.position.set(position[0], position[1], position[2]);
    mesh.rotation.set(0.15, Math.random() * Math.PI * 2, 0.1);
    mesh.userData.tool = tool;
    this.root.add(mesh);
    this.entries.push({
      availableAt: now + pickupDelayMs,
      baseY: position[1],
      mesh,
      quantity: 1,
      pickup: { kind: "tool", tool },
      vy: 2.6 + Math.random() * 0.6,
      settled: false,
      ownsGeometry: true,
    });
    void this.ensureTexture(cacheKey, TOOL_TEXTURE_URL[tool]).then(
      (texture) => {
        if (this.disposed || texture === null) return;
        if (material.map !== texture) {
          material.map = texture;
          material.color.set("#ffffff");
          material.needsUpdate = true;
        }
      },
    );
  }

  /**
   * @param accept 返回剩余数量（资源）；0 表示整份取走。工具应返回 0 表示拾取。
   */
  updateAndCollect(
    player: Vector3,
    now: number,
    deltaMs: number,
    accept: (pickup: LocalLootPickup) => number,
  ): void {
    const dt = Math.min(0.05, Math.max(0, deltaMs / 1_000));
    for (let index = this.entries.length - 1; index >= 0; index -= 1) {
      const entry = this.entries[index];
      entry.mesh.rotation.y += 0.025;

      if (!entry.settled) {
        entry.vy -= 14 * dt;
        entry.mesh.position.y += entry.vy * dt;
        if (entry.mesh.position.y <= entry.baseY) {
          entry.mesh.position.y = entry.baseY;
          entry.settled = true;
          entry.vy = 0;
        }
      } else {
        entry.mesh.position.y =
          entry.baseY + Math.sin(now * 0.004 + index) * 0.06;
      }

      if (
        now < entry.availableAt ||
        entry.mesh.position.distanceToSquared(player) > 2.25
      ) {
        continue;
      }
      const payload: LocalLootPickup =
        entry.pickup.kind === "resource"
          ? {
              kind: "resource",
              resource: entry.pickup.resource,
              quantity: entry.quantity,
            }
          : entry.pickup;
      const remainder = accept(payload);
      if (remainder > 0 && entry.pickup.kind === "resource") {
        entry.quantity = remainder;
        entry.availableAt = now + 350;
        continue;
      }
      this.entries.splice(index, 1);
      const material = entry.mesh.material;
      if (material instanceof MeshBasicMaterial) {
        // 贴图由 cache 共用，不 dispose map
        material.map = null;
        material.dispose();
      }
      if (entry.ownsGeometry) {
        entry.mesh.geometry.dispose();
      }
      this.root.remove(entry.mesh);
    }
  }

  dispose(): void {
    this.disposed = true;
    for (const entry of this.entries) {
      if (entry.ownsGeometry) entry.mesh.geometry.dispose();
      const material = entry.mesh.material;
      if (material instanceof MeshBasicMaterial) {
        material.map = null;
        material.dispose();
      }
    }
    this.entries.length = 0;
    for (const texture of this.textureCache.values()) {
      texture?.dispose();
    }
    this.textureCache.clear();
    this.sharedGeometry.dispose();
    disposeObjectTree(this.root, true);
  }

  private ensureTexture(
    cacheKey: string,
    url: string,
  ): Promise<Texture | null> {
    if (this.textureCache.has(cacheKey)) {
      return Promise.resolve(this.textureCache.get(cacheKey) ?? null);
    }
    return loadTexture(url)
      .then((texture) => {
        this.textureCache.set(cacheKey, texture);
        return texture;
      })
      .catch(() => {
        this.textureCache.set(cacheKey, null);
        return null;
      });
  }
}

function loadTexture(url: string): Promise<Texture> {
  return new Promise((resolve, reject) => {
    const image = new Image();
    image.decoding = "async";
    image.onload = () => {
      const texture = new Texture(image);
      texture.colorSpace = SRGBColorSpace;
      texture.magFilter = NearestFilter;
      texture.minFilter = NearestFilter;
      texture.generateMipmaps = false;
      texture.needsUpdate = true;
      resolve(texture);
    };
    image.onerror = () => reject(new Error(`无法加载掉落贴图 ${url}`));
    image.src = url;
  });
}
