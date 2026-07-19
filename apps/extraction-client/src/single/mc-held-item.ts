/**
 * 第三人称手持物：工具平面精灵 / 方块小立方，挂在右臂 heldSlot。
 * 位姿按像素单位（模型 scale=1/16 前）。
 */

import {
  BoxGeometry,
  DoubleSide,
  Mesh,
  MeshBasicMaterial,
  NearestFilter,
  PlaneGeometry,
  SRGBColorSpace,
  Texture,
} from "three";

import diamondOreUrl from "../assets/single/blocks/diamond_ore.png";
import dirtUrl from "../assets/single/blocks/dirt.png";
import goldOreUrl from "../assets/single/blocks/gold_ore.png";
import grassTopUrl from "../assets/single/blocks/grass_top.png";
import planksUrl from "../assets/single/blocks/oak_planks.png";
import stoneUrl from "../assets/single/blocks/stone.png";
import ironPickaxeUrl from "../assets/single/items/iron_pickaxe.png";
import ironSwordUrl from "../assets/single/items/iron_sword.png";

import type { LocalHeldContent } from "./held-content";
import type { LocalResourceKey } from "./state";

export type McHeldTool = "pickaxe" | "sword";

const ITEM_URLS: Record<McHeldTool, string> = {
  pickaxe: ironPickaxeUrl,
  sword: ironSwordUrl,
};

/** 与 HUD/掉落一致的资源图标贴图 */
export const HELD_BLOCK_TEXTURE_URL: Readonly<
  Record<LocalResourceKey, string>
> = {
  dirt: dirtUrl,
  grass: grassTopUrl,
  stone: stoneUrl,
  planks: planksUrl,
  leaves: grassTopUrl,
  gold: goldOreUrl,
  diamond: diamondOreUrl,
};

const HELD_BLOCK_FALLBACK: Readonly<Record<LocalResourceKey, string>> = {
  dirt: "#79523d",
  grass: "#5d8a3a",
  stone: "#8a8a8a",
  planks: "#9a7348",
  leaves: "#3d8c3a",
  gold: "#e0b43b",
  diamond: "#3dd2cc",
};

/** 加载并创建挂在右臂的手持平面（像素坐标系，约 8×8）。 */
export async function createMcHeldItemMesh(tool: McHeldTool): Promise<Mesh> {
  const texture = await loadNearestTexture(ITEM_URLS[tool]);
  const mesh = new Mesh(
    new PlaneGeometry(8, 8),
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
  mesh.name = `mc-held-${tool}`;
  mesh.frustumCulled = false;
  applyThirdPersonHeldPose(mesh, tool);
  return mesh;
}

/** 第三人称手持方块：小立方体贴方块面贴图 */
export async function createMcHeldBlockMesh(
  resource: LocalResourceKey,
): Promise<Mesh> {
  let texture: Texture | null = null;
  try {
    texture = await loadNearestTexture(HELD_BLOCK_TEXTURE_URL[resource]);
  } catch {
    texture = null;
  }
  const material = new MeshBasicMaterial({
    color: texture === null ? HELD_BLOCK_FALLBACK[resource] : "#ffffff",
    map: texture,
    transparent: texture !== null,
    alphaTest: texture !== null ? 0.05 : 0,
    depthTest: true,
    depthWrite: true,
    toneMapped: false,
  });
  // 约 5px 立方（模型 scale 1/16 → 世界约 0.31 格）
  const mesh = new Mesh(new BoxGeometry(5, 5, 5), material);
  mesh.name = `mc-held-block-${resource}`;
  mesh.frustumCulled = false;
  applyThirdPersonBlockPose(mesh);
  return mesh;
}

/** 按手持内容创建第三人称 mesh（empty → null） */
export async function createMcHeldContentMesh(
  content: LocalHeldContent,
): Promise<Mesh | null> {
  if (content.kind === "empty") return null;
  if (content.kind === "tool") return createMcHeldItemMesh(content.tool);
  return createMcHeldBlockMesh(content.resource);
}

/**
 * 第三人称工具握持位姿。
 * heldSlot 原点 = 手心；局部 +Y 朝肩，-Y 朝指尖。
 */
export function applyThirdPersonHeldPose(mesh: Mesh, tool: McHeldTool): void {
  mesh.rotation.order = "YXZ";

  if (tool === "sword") {
    mesh.rotation.set(-0.55, Math.PI * 0.5, Math.PI * 0.25 + Math.PI);
    mesh.position.set(0.4, -1.2, -1.8);
  } else {
    mesh.rotation.set(-0.4, Math.PI * 0.5, Math.PI * 0.2 + Math.PI);
    mesh.position.set(0.5, -1.0, -1.6);
  }
}

/** 方块握在手心（像素坐标，贴 heldSlot） */
export function applyThirdPersonBlockPose(mesh: Mesh): void {
  mesh.rotation.order = "YXZ";
  // 小立方贴在指尖，避免第三人称低头时「漂」在身体旁
  mesh.rotation.set(-0.2, 0.5, 0.05);
  mesh.position.set(0.3, -9.2, -0.8);
  mesh.scale.set(0.9, 0.9, 0.9);
}

export function disposeMcHeldItemMesh(mesh: Mesh): void {
  mesh.geometry.dispose();
  const material = mesh.material;
  if (material instanceof MeshBasicMaterial) {
    material.map?.dispose();
    material.dispose();
  }
}

function loadNearestTexture(url: string): Promise<Texture> {
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
