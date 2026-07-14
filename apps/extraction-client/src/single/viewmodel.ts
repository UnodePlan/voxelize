import { Arm } from "@voxelize/core";
import {
  AmbientLight,
  CanvasTexture,
  DoubleSide,
  Mesh,
  MeshBasicMaterial,
  NearestFilter,
  PlaneGeometry,
  Quaternion,
  SRGBColorSpace,
  Scene,
  Texture,
  Vector3,
  type PerspectiveCamera,
} from "three";

import ironPickaxeUrl from "../assets/single/items/iron_pickaxe.png";
import ironSwordUrl from "../assets/single/items/iron_sword.png";

import { disposeObjectTree } from "../game/object-disposal";

/** 第一人称握持：空手手臂 / 镐 / 剑（对齐 lab Arm 三态） */
export type LocalHeldTool = "empty" | "pickaxe" | "sword";

/**
 * create.town/lab：Voxelize Arm + 独立 armScene。
 * - empty → setArmObject(undefined) 显示皮肤手臂
 * - pickaxe/sword → 16×16 平面精灵 + customType "item"
 */
export class LocalViewmodel {
  readonly arm: Arm;
  readonly scene = new Scene();
  private held: LocalHeldTool = "empty";
  private itemMeshes = new Map<Exclude<LocalHeldTool, "empty">, Mesh>();
  private lastSwingAt = 0;
  private wasMining = false;
  private disposed = false;
  private equipGeneration = 0;

  constructor(private readonly armCamera: PerspectiveCamera) {
    const armTexture = createArmTexture();
    this.arm = new Arm({
      armTexture,
      armColor: "#c48a64",
      receiveShadows: false,
      receiveHeldObjectShadows: false,
      minOccluderDepth: 0.04,
      customObjectOptions: {
        // lab 3D item 位姿；平面精灵在 equipItem 里再覆盖
        item: {
          position: new Vector3(0.9, -0.7, -1.15),
          quaternion: new Quaternion().identity(),
        },
      },
    });
    this.arm.heldLightColor.setRGB(1, 1, 1);
    this.scene.add(new AmbientLight(0xffffff, 1));
    this.scene.add(this.arm);
    // 默认空手手臂（Arm 构造已 setArm）；预加载镐/剑 mesh
    void this.preloadItems();
  }

  get heldTool(): LocalHeldTool {
    return this.held;
  }

  /**
   * 切换握持物。animate=true 时用 lab 的升降过渡。
   * 同 tool 也会在 mesh 刚预加载完成时再次调用以真正 equip。
   */
  setHeldTool(tool: LocalHeldTool, animate = true): void {
    if (this.disposed) return;
    const previous = this.held;
    this.held = tool;
    if (tool === "empty") {
      if (previous !== "empty") this.arm.setArmObject(undefined, animate);
      return;
    }
    const mesh = this.itemMeshes.get(tool);
    if (mesh === undefined) {
      // 贴图尚未就绪：先空手，preload 结束后会再 equip
      if (previous !== "empty") this.arm.setArmObject(undefined, animate);
      return;
    }
    // 同工具补 equip（贴图刚就绪）不播切换动画
    this.arm.setArmObject(mesh, animate && previous !== tool, "item");
    applyItemSpritePose(mesh);
  }

  setMiningProgress(progress: number | null): void {
    // 任意握持物在持续挖掘时都跟挥动（空手/镐/剑）
    const mining = progress !== null;
    const interval =
      this.held === "pickaxe" ? 200 : this.held === "sword" ? 240 : 280;
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
    for (const mesh of this.itemMeshes.values()) {
      mesh.geometry.dispose();
      const material = mesh.material;
      if (material instanceof MeshBasicMaterial) {
        material.map?.dispose();
        material.dispose();
      }
    }
    this.itemMeshes.clear();
    disposeObjectTree(this.arm, true);
    this.scene.clear();
  }

  private async preloadItems(): Promise<void> {
    const generation = ++this.equipGeneration;
    const specs: Array<{
      tool: Exclude<LocalHeldTool, "empty">;
      url: string;
    }> = [
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
          const mesh = createItemSprite(texture);
          this.itemMeshes.set(tool, mesh);
        } catch (error) {
          console.warn(`[single] 手持物贴图加载失败: ${tool}`, error);
        }
      }),
    );
    // 若等待期间已选中镐/剑，补 equip
    if (!this.disposed && this.held !== "empty") {
      this.setHeldTool(this.held, false);
    }
  }
}

/** 快捷栏槽位 → 握持物（0 空手，1 镐，2 剑；资源槽仍显示空手手臂） */
export function heldToolFromSlot(slot: number): LocalHeldTool {
  if (slot === 1) return "pickaxe";
  if (slot === 2) return "sword";
  // 0 与资源槽（3+）均空手；资源不占用前三工具格
  return "empty";
}

function createItemSprite(texture: Texture): Mesh {
  const mesh = new Mesh(
    new PlaneGeometry(1.15, 1.15),
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
  mesh.frustumCulled = false;
  mesh.renderOrder = 20;
  applyItemSpritePose(mesh);
  return mesh;
}

/** 平面精灵面向镜头的第一人称位姿（镐/剑共用） */
function applyItemSpritePose(mesh: Mesh): void {
  mesh.position.set(0.72, -0.52, -0.95);
  mesh.rotation.set(0.18, Math.PI * 0.82, 0.42);
  mesh.scale.set(1, 1, 1);
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
  // 袖口深色条，接近 MC 手臂分块
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
