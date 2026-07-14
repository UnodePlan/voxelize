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

import { disposeObjectTree } from "../game/object-disposal";

/**
 * 对齐 create.town/lab：Voxelize Arm + 独立 armScene；
 * 手持镐使用 Lab 搬运的 16×16 iron_pickaxe 精灵贴图。
 */
export class LocalViewmodel {
  readonly arm: Arm;
  readonly scene = new Scene();
  private pickaxe: Mesh | null = null;
  private lastSwingAt = 0;
  private wasMining = false;
  private disposed = false;

  constructor(private readonly armCamera: PerspectiveCamera) {
    const armTexture = createArmTexture();
    // create.town: customObjectOptions.item 位姿（解包值）
    this.arm = new Arm({
      armTexture,
      armColor: "#c48a64",
      receiveShadows: false,
      receiveHeldObjectShadows: false,
      minOccluderDepth: 0.04,
      customObjectOptions: {
        // 平面精灵专用位姿：先占位，equip 后再调到面向镜头
        item: {
          position: new Vector3(0.9, -0.7, -1.15),
          quaternion: new Quaternion().identity(),
        },
      },
    });
    this.arm.heldLightColor.setRGB(1, 1, 1);
    this.scene.add(new AmbientLight(0xffffff, 1));
    this.scene.add(this.arm);
    void this.equipLabPickaxe();
  }

  setMiningProgress(progress: number | null): void {
    const mining = progress !== null;
    if (
      mining &&
      (!this.wasMining || performance.now() - this.lastSwingAt > 280)
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
    if (this.pickaxe !== null) {
      this.pickaxe.geometry.dispose();
      const material = this.pickaxe.material;
      if (material instanceof MeshBasicMaterial) {
        material.map?.dispose();
        material.dispose();
      }
      this.pickaxe = null;
    }
    disposeObjectTree(this.arm, true);
    this.scene.clear();
  }

  private async equipLabPickaxe(): Promise<void> {
    try {
      const texture = await loadItemTexture(ironPickaxeUrl);
      if (this.disposed) {
        texture.dispose();
        return;
      }
      // 16×16 物品精灵：双面薄板 + 透明抠空，第一人称右下角面向镜头
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
      this.pickaxe = mesh;
      this.arm.setArmObject(mesh, false, "item");
      // setArmObject 之后覆盖为精灵友好朝向（Lab 的 3D item 四元数会让平面侧立）
      mesh.position.set(0.72, -0.52, -0.95);
      mesh.rotation.set(0.18, Math.PI * 0.82, 0.42);
      mesh.scale.set(1, 1, 1);
    } catch (error) {
      console.warn("[single] Lab 镐贴图加载失败，保留默认手臂", error);
    }
  }
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
