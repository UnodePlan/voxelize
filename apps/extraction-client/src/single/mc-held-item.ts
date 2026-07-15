/**
 * 第三人称 MC 手持物：16×16 平面精灵挂在右臂末端。
 * 与第一人称 viewmodel 共用贴图，位姿按像素单位（模型 scale 前）。
 *
 * 贴图约定（Lab/MC item icon）：
 * - 柄在贴图左下角附近
 * - 刃/镐头朝右上
 * 握持时必须让「柄」落在 heldSlot（手心），刃/头朝上前方。
 */

import {
  DoubleSide,
  Mesh,
  MeshBasicMaterial,
  NearestFilter,
  PlaneGeometry,
  SRGBColorSpace,
  Texture,
} from "three";

import ironPickaxeUrl from "../assets/single/items/iron_pickaxe.png";
import ironSwordUrl from "../assets/single/items/iron_sword.png";

export type McHeldTool = "pickaxe" | "sword";

const ITEM_URLS: Record<McHeldTool, string> = {
  pickaxe: ironPickaxeUrl,
  sword: ironSwordUrl,
};

/** 加载并创建挂在右臂的手持平面（像素坐标系，约 8×8）。 */
export async function createMcHeldItemMesh(
  tool: McHeldTool,
): Promise<Mesh> {
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

/**
 * 第三人称握持位姿。
 * heldSlot 原点 = 手心（rightArm 末端）；局部 +Y 沿手臂朝肩，-Y 朝指尖方向
 * （arm 下垂时）。
 *
 * Plane 默认朝 +Z；贴图 UV 左下 = 柄。先把柄旋到手心侧，再让刃朝上前。
 */
export function applyThirdPersonHeldPose(
  mesh: Mesh,
  tool: McHeldTool,
): void {
  // 旋转顺序 YXZ，避免欧拉万向节把「翻柄」拧乱
  mesh.rotation.order = "YXZ";

  if (tool === "sword") {
    // 剑：柄在手，刃朝上偏前（原版第三人称斜握）
    // 关键：+π 把原先「握刃」翻成「握柄」
    mesh.rotation.set(
      -0.55, // 刃略抬起
      Math.PI * 0.5, // 贴图平面朝向身体侧方，避免只看到薄边
      Math.PI * 0.25 + Math.PI, // 对角 + 翻柄
    );
    // 平面中心在柄偏上；往指尖/前移，让柄落入手心
    mesh.position.set(0.4, -1.2, -1.8);
  } else {
    // 镐：柄在手，镐头朝上前（比剑更「竖」一点）
    mesh.rotation.set(
      -0.4,
      Math.PI * 0.5,
      Math.PI * 0.2 + Math.PI,
    );
    mesh.position.set(0.5, -1.0, -1.6);
  }
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
