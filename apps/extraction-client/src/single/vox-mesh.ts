/**
 * VOX 体素 → three.js Mesh（合并立方体，顶点色）。
 * MagicaVoxel Z-up → Three.js Y-up： (x,y,z) → (x, z, y)
 */

import {
  BoxGeometry,
  BufferAttribute,
  BufferGeometry,
  Color,
  Mesh,
  MeshLambertMaterial,
  type Group,
} from "three";
import { mergeGeometries } from "three/examples/jsm/utils/BufferGeometryUtils.js";

import type { VoxModel } from "./vox-loader";

export interface VoxMeshOptions {
  /** 单个体素边长（世界单位） */
  voxelSize?: number;
  /** 是否绕局部 Y 镜像（用于左右手/脚） */
  mirrorX?: boolean;
}

/**
 * 将 VOX 转为 Mesh；几何中心落在原点附近（按包围盒中心对齐）。
 */
export function voxModelToMesh(
  model: VoxModel,
  options: VoxMeshOptions = {},
): Mesh {
  const voxelSize = options.voxelSize ?? 0.1;
  const mirrorX = options.mirrorX ?? false;
  const geos: BufferGeometry[] = [];
  const unit = new BoxGeometry(voxelSize, voxelSize, voxelSize);

  // 包围盒（Three 空间）
  let minX = Infinity;
  let minY = Infinity;
  let minZ = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  let maxZ = -Infinity;

  type Placed = {
    x: number;
    y: number;
    z: number;
    r: number;
    g: number;
    b: number;
  };
  const placed: Placed[] = [];

  for (const v of model.voxels) {
    const color = model.palette[v.colorIndex] ?? [200, 200, 200, 255];
    if (color[3] < 8) continue;
    // Magica → Three
    let x = (v.x + 0.5) * voxelSize;
    const y = (v.z + 0.5) * voxelSize;
    const z = (v.y + 0.5) * voxelSize;
    if (mirrorX) x = -x;
    placed.push({
      x,
      y,
      z,
      r: color[0] / 255,
      g: color[1] / 255,
      b: color[2] / 255,
    });
    minX = Math.min(minX, x);
    minY = Math.min(minY, y);
    minZ = Math.min(minZ, z);
    maxX = Math.max(maxX, x);
    maxY = Math.max(maxY, y);
    maxZ = Math.max(maxZ, z);
  }

  if (placed.length === 0) {
    unit.dispose();
    const empty = new Mesh(
      new BoxGeometry(voxelSize, voxelSize, voxelSize),
      new MeshLambertMaterial({ color: 0x888888 }),
    );
    return empty;
  }

  const cx = (minX + maxX) / 2;
  const cy = (minY + maxY) / 2;
  const cz = (minZ + maxZ) / 2;

  for (const p of placed) {
    const g = unit.clone();
    g.translate(p.x - cx, p.y - cy, p.z - cz);
    const colors = new Float32Array(g.attributes.position.count * 3);
    for (let i = 0; i < g.attributes.position.count; i += 1) {
      colors[i * 3] = p.r;
      colors[i * 3 + 1] = p.g;
      colors[i * 3 + 2] = p.b;
    }
    g.setAttribute("color", new BufferAttribute(colors, 3));
    geos.push(g);
  }
  unit.dispose();

  const merged = mergeGeometries(geos, false);
  for (const g of geos) g.dispose();
  if (merged === null) {
    throw new Error("合并 VOX 几何失败");
  }
  merged.computeVertexNormals();

  const material = new MeshLambertMaterial({
    vertexColors: true,
  });
  const mesh = new Mesh(merged, material);
  // 记录半高，便于装腿
  mesh.userData.halfHeight = (maxY - minY) / 2;
  mesh.userData.size = {
    x: maxX - minX + voxelSize,
    y: maxY - minY + voxelSize,
    z: maxZ - minZ + voxelSize,
  };
  return mesh;
}

export function disposeObjectMesh(root: Group | Mesh): void {
  root.traverse((obj) => {
    if (obj instanceof Mesh) {
      obj.geometry?.dispose();
      const mat = obj.material;
      if (Array.isArray(mat)) mat.forEach((m) => m.dispose());
      else mat?.dispose();
    }
  });
}

/** 调试用：把 mesh 染成单色 */
export function tintMesh(mesh: Mesh, hex: number): void {
  const c = new Color(hex);
  const geo = mesh.geometry;
  const count = geo.attributes.position.count;
  const colors = new Float32Array(count * 3);
  for (let i = 0; i < count; i += 1) {
    colors[i * 3] = c.r;
    colors[i * 3 + 1] = c.g;
    colors[i * 3 + 2] = c.b;
  }
  geo.setAttribute("color", new BufferAttribute(colors, 3));
}
