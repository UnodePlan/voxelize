import {
  BoxGeometry,
  CanvasTexture,
  DoubleSide,
  Group,
  Mesh,
  MeshBasicMaterial,
  NearestFilter,
  SRGBColorSpace,
  type Object3D,
  type Texture,
} from "three";

import { disposeObjectTree } from "../game/object-disposal";

/** 原版 destroy_stage 共 10 级（0–9） */
export const BREAK_CRACK_STAGES = 10;

interface DebrisPiece {
  ageMs: number;
  lifeMs: number;
  mesh: Mesh;
  vx: number;
  vy: number;
  vz: number;
}

/**
 * MC 风格挖掘反馈：
 * - 目标方块上的裂纹 overlay（随进度 0–9 级）
 * - 破坏时弹出的短暂碎片
 */
export class BlockBreakFx {
  private readonly root = new Group();
  private readonly crackMesh: Mesh;
  private readonly crackMaterials: MeshBasicMaterial[];
  private readonly debris: DebrisPiece[] = [];
  private readonly sharedBox = new BoxGeometry(1.002, 1.002, 1.002);
  private readonly debrisBox = new BoxGeometry(0.18, 0.18, 0.18);
  private activeStage = -1;
  private disposed = false;

  constructor(world: Object3D) {
    this.root.name = "single-block-break-fx";
    world.add(this.root);

    this.crackMaterials = createCrackStageMaterials();
    this.crackMesh = new Mesh(this.sharedBox, this.crackMaterials[0]);
    this.crackMesh.visible = false;
    this.crackMesh.renderOrder = 8;
    this.crackMesh.frustumCulled = false;
    // 略抬出表面，避免 z-fight
    this.crackMesh.scale.setScalar(1.01);
    this.root.add(this.crackMesh);
  }

  /**
   * @param progress 0–1 挖掘进度；null 隐藏裂纹
   * @param voxel 目标方块角点
   */
  setCrack(
    progress: number | null,
    voxel: readonly [number, number, number] | null,
  ): void {
    if (this.disposed) return;
    if (
      progress === null ||
      voxel === null ||
      !Number.isFinite(progress) ||
      progress <= 0
    ) {
      this.crackMesh.visible = false;
      this.activeStage = -1;
      return;
    }
    const stage = Math.min(
      BREAK_CRACK_STAGES - 1,
      Math.floor(progress * BREAK_CRACK_STAGES),
    );
    if (stage !== this.activeStage) {
      this.crackMesh.material = this.crackMaterials[stage];
      this.activeStage = stage;
    }
    this.crackMesh.position.set(voxel[0] + 0.5, voxel[1] + 0.5, voxel[2] + 0.5);
    this.crackMesh.visible = true;
  }

  /** 方块打碎时的短生命周期碎片 */
  burst(
    voxel: readonly [number, number, number],
    color: string,
    now: number,
  ): void {
    if (this.disposed) return;
    const cx = voxel[0] + 0.5;
    const cy = voxel[1] + 0.5;
    const cz = voxel[2] + 0.5;
    const count = 6;
    for (let i = 0; i < count; i += 1) {
      const material = new MeshBasicMaterial({
        color,
        transparent: true,
        opacity: 0.95,
        depthWrite: false,
      });
      const mesh = new Mesh(this.debrisBox, material);
      mesh.position.set(
        cx + (Math.random() - 0.5) * 0.35,
        cy + (Math.random() - 0.5) * 0.35,
        cz + (Math.random() - 0.5) * 0.35,
      );
      mesh.rotation.set(
        Math.random() * Math.PI,
        Math.random() * Math.PI,
        Math.random() * Math.PI,
      );
      this.root.add(mesh);
      this.debris.push({
        ageMs: 0,
        lifeMs: 380 + Math.random() * 220,
        mesh,
        vx: (Math.random() - 0.5) * 3.2,
        vy: 1.6 + Math.random() * 2.2,
        vz: (Math.random() - 0.5) * 3.2,
      });
    }
    void now;
  }

  update(deltaMs: number): void {
    if (this.disposed || this.debris.length === 0) return;
    const dt = Math.min(0.05, Math.max(0, deltaMs / 1_000));
    for (let i = this.debris.length - 1; i >= 0; i -= 1) {
      const piece = this.debris[i];
      piece.ageMs += deltaMs;
      piece.vy -= 12 * dt;
      piece.mesh.position.x += piece.vx * dt;
      piece.mesh.position.y += piece.vy * dt;
      piece.mesh.position.z += piece.vz * dt;
      piece.mesh.rotation.x += dt * 6;
      piece.mesh.rotation.y += dt * 8;
      const life = piece.ageMs / piece.lifeMs;
      const material = piece.mesh.material;
      if (material instanceof MeshBasicMaterial) {
        material.opacity = Math.max(0, 0.95 * (1 - life));
      }
      if (piece.ageMs >= piece.lifeMs) {
        this.root.remove(piece.mesh);
        if (material instanceof MeshBasicMaterial) material.dispose();
        this.debris.splice(i, 1);
      }
    }
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.debris.length = 0;
    for (const material of this.crackMaterials) {
      material.map?.dispose();
      material.dispose();
    }
    this.sharedBox.dispose();
    this.debrisBox.dispose();
    disposeObjectTree(this.root, true);
  }
}

function createCrackStageMaterials(): MeshBasicMaterial[] {
  const textures = createCrackStageTextures();
  return textures.map(
    (map) =>
      new MeshBasicMaterial({
        map,
        transparent: true,
        opacity: 1,
        depthWrite: false,
        side: DoubleSide,
        polygonOffset: true,
        polygonOffsetFactor: -2,
        polygonOffsetUnits: -2,
        toneMapped: false,
      }),
  );
}

/** 程序化生成 10 级裂纹（近似原版 destroy_stage 递增碎裂） */
function createCrackStageTextures(): Texture[] {
  const size = 16;
  const textures: Texture[] = [];
  for (let stage = 0; stage < BREAK_CRACK_STAGES; stage += 1) {
    const canvas = document.createElement("canvas");
    canvas.width = size;
    canvas.height = size;
    const ctx = canvas.getContext("2d");
    if (ctx === null) throw new Error("无法创建裂纹纹理");
    ctx.imageSmoothingEnabled = false;
    ctx.clearRect(0, 0, size, size);

    // 裂纹条数与阶段递增；深色半透明，叠在方块表面
    const lines = 2 + stage;
    ctx.strokeStyle = `rgba(20, 18, 16, ${0.35 + stage * 0.06})`;
    ctx.lineWidth = 1;
    for (let i = 0; i < lines; i += 1) {
      const seed = stage * 17 + i * 31;
      let x = pseudo(seed) * size;
      let y = pseudo(seed + 3) * size;
      ctx.beginPath();
      ctx.moveTo(x, y);
      const segs = 3 + Math.floor(stage / 2);
      for (let s = 0; s < segs; s += 1) {
        x += (pseudo(seed + s * 7 + 1) - 0.5) * (4 + stage * 0.6);
        y += (pseudo(seed + s * 11 + 2) - 0.5) * (4 + stage * 0.6);
        ctx.lineTo(
          Math.max(0, Math.min(size - 1, x)),
          Math.max(0, Math.min(size - 1, y)),
        );
      }
      ctx.stroke();
    }
    // 高阶段加几条交叉裂口
    if (stage >= 4) {
      ctx.fillStyle = `rgba(0, 0, 0, ${0.08 + (stage - 4) * 0.04})`;
      for (let i = 0; i < stage - 2; i += 1) {
        const px = Math.floor(pseudo(stage * 40 + i) * size);
        const py = Math.floor(pseudo(stage * 40 + i + 9) * size);
        ctx.fillRect(px, py, 1, 1 + (i % 2));
      }
    }

    const texture = new CanvasTexture(canvas);
    texture.colorSpace = SRGBColorSpace;
    texture.magFilter = NearestFilter;
    texture.minFilter = NearestFilter;
    texture.generateMipmaps = false;
    texture.needsUpdate = true;
    textures.push(texture);
  }
  return textures;
}

function pseudo(n: number): number {
  const x = Math.sin(n * 12.9898) * 43758.5453;
  return x - Math.floor(x);
}
