import { Arm } from "@voxelize/core";
import {
  AmbientLight,
  BoxGeometry,
  CanvasTexture,
  DoubleSide,
  Euler,
  Group,
  Mesh,
  MeshBasicMaterial,
  NearestFilter,
  PlaneGeometry,
  Quaternion,
  SRGBColorSpace,
  Scene,
  Texture,
  Vector3,
  type Object3D,
  type PerspectiveCamera,
} from "three";

import ironPickaxeUrl from "../assets/single/items/iron_pickaxe.png";
import ironSwordUrl from "../assets/single/items/iron_sword.png";
import { disposeObjectTree } from "../game/object-disposal";

import { heldContentEquals, type LocalHeldContent } from "./held-content";
import { HELD_BLOCK_TEXTURE_URL } from "./mc-held-item";
import type { LocalResourceKey } from "./state";

/** 战斗/挖掘用握持工具（资源槽不算工具） */
export type LocalHeldTool = "empty" | "pickaxe" | "sword";

/**
 * 持物组世界位姿（方块：手臂 + 方块）。
 * - 旋转：与空手 ARM 相同（前倾+外展）
 * - 位置：比空手略远，避免 FOV 裁切
 */
const HELD_ARM_POSITION = new Vector3(1.0, -0.88, -1.5);
const HELD_ARM_QUATERNION = new Quaternion().setFromEuler(
  new Euler(-Math.PI / 4, 0, -Math.PI / 8),
);

/**
 * create.town/lab：Voxelize Arm + 独立 armScene。
 * - empty → 官方 CanvasBox 手臂
 * - tool → 16×16 平面精灵 + customType "item"（无嵌套手臂，避免遮挡）
 * - block → customType "held"：同空手位姿的手臂 + 末端方块
 */
export class LocalViewmodel {
  readonly arm: Arm;
  readonly scene = new Scene();
  private held: LocalHeldContent = { kind: "empty" };
  /** 镐/剑：仅平面精灵 */
  private toolMeshes = new Map<"pickaxe" | "sword", Mesh>();
  /** 资源：手臂 + 方块组 */
  private blockArmGroups = new Map<LocalResourceKey, Group>();
  private armSkinTexture: CanvasTexture;
  private lastSwingAt = 0;
  private wasMining = false;
  private disposed = false;
  private equipGeneration = 0;

  constructor(private readonly armCamera: PerspectiveCamera) {
    this.armSkinTexture = createArmTexture();
    this.arm = new Arm({
      armTexture: this.armSkinTexture,
      armColor: "#c48a64",
      receiveShadows: false,
      receiveHeldObjectShadows: false,
      minOccluderDepth: 0.04,
      customObjectOptions: {
        // 工具精灵：位姿由 applyItemSpritePose 覆盖
        item: {
          position: new Vector3(0.9, -0.7, -1.15),
          quaternion: new Quaternion().identity(),
        },
        // 方块：手臂 + 方块共用世界位姿
        held: {
          position: HELD_ARM_POSITION.clone(),
          quaternion: HELD_ARM_QUATERNION.clone(),
        },
      },
    });
    this.arm.heldLightColor.setRGB(1, 1, 1);
    this.scene.add(new AmbientLight(0xffffff, 1));
    this.scene.add(this.arm);
    void this.preloadTools();
    void this.preloadBlocks();
  }

  get heldContent(): LocalHeldContent {
    return this.held;
  }

  get heldTool(): LocalHeldTool {
    if (this.held.kind === "tool") return this.held.tool;
    return "empty";
  }

  setHeldContent(content: LocalHeldContent, animate = true): void {
    if (this.disposed) return;
    const previous = this.held;
    this.held = content;
    if (content.kind === "empty") {
      if (previous.kind !== "empty") this.arm.setArmObject(undefined, animate);
      return;
    }

    const object = this.resolveObject(content);
    if (object === null) {
      if (previous.kind !== "empty") this.arm.setArmObject(undefined, animate);
      return;
    }

    const same = heldContentEquals(previous, content);
    // setCustomObject 会对 quaternion 做 multiply，每次 equip 前归零
    resetObjectTransform(object);
    if (content.kind === "tool") {
      this.arm.setArmObject(object, animate && !same, "item");
      applyItemSpritePose(object as Mesh, content.tool);
    } else {
      this.arm.setArmObject(object, animate && !same, "held");
    }
  }

  setHeldTool(tool: LocalHeldTool, animate = true): void {
    if (tool === "empty") {
      this.setHeldContent({ kind: "empty" }, animate);
    } else {
      this.setHeldContent({ kind: "tool", tool }, animate);
    }
  }

  setMiningProgress(progress: number | null): void {
    const mining = progress !== null;
    const interval =
      this.held.kind === "tool" && this.held.tool === "pickaxe"
        ? 200
        : this.held.kind === "tool" && this.held.tool === "sword"
          ? 240
          : 260;
    if (
      mining &&
      (!this.wasMining || performance.now() - this.lastSwingAt > interval)
    ) {
      this.arm.doSwing();
      this.lastSwingAt = performance.now();
    }
    this.wasMining = mining;
  }

  resize(width: number, height: number): void {
    this.armCamera.aspect = Math.max(width / Math.max(height, 1), 0.01);
    this.armCamera.updateProjectionMatrix();
  }

  dispose(): void {
    this.disposed = true;
    this.equipGeneration += 1;
    for (const mesh of this.toolMeshes.values()) {
      mesh.geometry.dispose();
      const material = mesh.material;
      if (material instanceof MeshBasicMaterial) {
        material.map?.dispose();
        material.dispose();
      }
    }
    for (const group of this.blockArmGroups.values()) {
      disposeObjectTree(group, false);
    }
    this.toolMeshes.clear();
    this.blockArmGroups.clear();
    this.armSkinTexture.dispose();
    disposeObjectTree(this.arm, true);
    this.scene.clear();
  }

  private resolveObject(content: LocalHeldContent): Object3D | null {
    if (content.kind === "tool") {
      return this.toolMeshes.get(content.tool) ?? null;
    }
    if (content.kind === "block") {
      return this.blockArmGroups.get(content.resource) ?? null;
    }
    return null;
  }

  private async preloadTools(): Promise<void> {
    const generation = ++this.equipGeneration;
    const specs: Array<{ tool: "pickaxe" | "sword"; url: string }> = [
      { tool: "pickaxe", url: ironPickaxeUrl },
      { tool: "sword", url: ironSwordUrl },
    ];
    await Promise.all(
      specs.map(async ({ tool, url }) => {
        try {
          const texture = await loadItemTexture(url);
          if (this.disposed || generation !== this.equipGeneration) {
            texture.dispose();
            return;
          }
          this.toolMeshes.set(tool, createItemSprite(texture, tool));
        } catch (error) {
          console.warn(`[single] 手持工具贴图加载失败: ${tool}`, error);
        }
      }),
    );
    this.reEquipIfNeeded();
  }

  private async preloadBlocks(): Promise<void> {
    const generation = this.equipGeneration;
    const resources = Object.keys(HELD_BLOCK_TEXTURE_URL) as LocalResourceKey[];
    await Promise.all(
      resources.map(async (resource) => {
        try {
          const texture = await loadItemTexture(
            HELD_BLOCK_TEXTURE_URL[resource],
          );
          if (this.disposed || generation !== this.equipGeneration) {
            texture.dispose();
            return;
          }
          this.blockArmGroups.set(
            resource,
            createHeldBlockArm(this.armSkinTexture, resource, texture),
          );
        } catch (error) {
          console.warn(`[single] 手持方块贴图加载失败: ${resource}`, error);
        }
      }),
    );
    this.reEquipIfNeeded();
  }

  private reEquipIfNeeded(): void {
    if (this.disposed || this.held.kind === "empty") return;
    if (this.resolveObject(this.held) !== null) {
      this.setHeldContent(this.held, false);
    }
  }
}

export function heldToolFromSlot(slot: number): LocalHeldTool {
  if (slot === 1) return "pickaxe";
  if (slot === 2) return "sword";
  return "empty";
}

function createItemSprite(texture: Texture, tool: "pickaxe" | "sword"): Mesh {
  const mesh = new Mesh(
    // 略放大，细长剑/镐在 FOV 边缘仍可辨认
    new PlaneGeometry(1.2, 1.2),
    new MeshBasicMaterial({
      map: texture,
      transparent: true,
      alphaTest: 0.12,
      depthTest: true,
      depthWrite: false,
      side: DoubleSide,
      toneMapped: false,
    }),
  );
  mesh.name = `fp-held-${tool}`;
  mesh.frustumCulled = false;
  mesh.renderOrder = 20;
  applyItemSpritePose(mesh, tool);
  return mesh;
}

/**
 * 平面精灵面向镜头的第一人称位姿。
 * 不嵌套手臂几何，避免薄精灵被手臂 depth 遮挡。
 */
function applyItemSpritePose(mesh: Mesh, tool: "pickaxe" | "sword"): void {
  mesh.position.set(0.72, -0.52, -0.95);
  // 剑略更竖，镐略更斜
  mesh.rotation.set(
    0.18,
    Math.PI * (tool === "sword" ? 0.78 : 0.82),
    tool === "sword" ? 0.35 : 0.42,
  );
  mesh.scale.set(1, 1, 1);
}

/**
 * 局部坐标：+Y 肩侧，-Y 指尖；几何对齐官方 setArm CanvasBox。
 */
function createHeldBlockArm(
  _armTexture: Texture,
  resource: LocalResourceKey,
  texture: Texture,
): Group {
  const group = new Group();
  group.name = `fp-held-block-arm-${resource}`;

  const arm = new Mesh(
    new BoxGeometry(0.5, 1, 0.3),
    new MeshBasicMaterial({
      color: "#c48a64",
      toneMapped: false,
    }),
  );
  arm.name = "fp-right-arm";
  arm.frustumCulled = false;

  const cuff = new Mesh(
    new BoxGeometry(0.52, 0.16, 0.32),
    new MeshBasicMaterial({ color: "#3f4f55", toneMapped: false }),
  );
  cuff.name = "fp-right-cuff";
  cuff.frustumCulled = false;
  cuff.position.set(0, 0.42, 0);

  // 方块贴在掌心外侧，略伸出前臂以免被深度遮挡
  const block = new Mesh(
    new BoxGeometry(0.4, 0.4, 0.4),
    new MeshBasicMaterial({
      map: texture,
      toneMapped: false,
    }),
  );
  block.name = "fp-held-block";
  block.frustumCulled = false;
  block.position.set(0.22, -0.42, 0.12);
  block.rotation.set(0.15, -0.45, 0.12);

  group.add(arm, cuff, block);
  return group;
}

function resetObjectTransform(object: Object3D): void {
  object.position.set(0, 0, 0);
  object.rotation.set(0, 0, 0);
  object.quaternion.identity();
  object.scale.set(1, 1, 1);
}

function createArmTexture(): CanvasTexture {
  const canvas = document.createElement("canvas");
  canvas.width = 16;
  canvas.height = 16;
  const context = canvas.getContext("2d");
  if (context === null) throw new Error("无法创建手臂纹理");
  context.imageSmoothingEnabled = false;
  context.fillStyle = "#c48a64";
  context.fillRect(0, 0, 16, 16);
  context.fillStyle = "#b57a56";
  for (let y = 0; y < 16; y += 1) {
    for (let x = 0; x < 16; x += 1) {
      if ((x * 13 + y * 7) % 11 === 0) context.fillRect(x, y, 1, 1);
    }
  }
  context.fillStyle = "#3f4f55";
  context.fillRect(0, 0, 16, 3);
  const texture = new CanvasTexture(canvas);
  texture.colorSpace = SRGBColorSpace;
  texture.magFilter = NearestFilter;
  texture.minFilter = NearestFilter;
  texture.generateMipmaps = false;
  texture.needsUpdate = true;
  return texture;
}

function loadItemTexture(url: string): Promise<Texture> {
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
    image.onerror = () => reject(new Error(`无法加载手持物贴图 ${url}`));
    image.src = url;
  });
}
