import type { Material, Mesh, Object3D, Texture } from "three";

export function disposeObjectTree(
  root: Object3D,
  disposeTextures: boolean,
): void {
  const geometries = new Set<Mesh["geometry"]>();
  const materials = new Set<Material>();
  const textures = new Set<Texture>();
  root.traverse((object) => {
    const mesh = object as Mesh;
    if (!mesh.isMesh) return;
    if (!geometries.has(mesh.geometry)) {
      geometries.add(mesh.geometry);
      mesh.geometry.dispose();
    }
    const ownedMaterials = Array.isArray(mesh.material)
      ? mesh.material
      : [mesh.material];
    ownedMaterials.forEach((material) => {
      if (materials.has(material)) return;
      materials.add(material);
      if (disposeTextures) disposeMaterialTextures(material, textures);
      material.dispose();
    });
  });
  root.removeFromParent();
  root.clear();
}

function disposeMaterialTextures(
  material: Material,
  disposed: Set<Texture>,
): void {
  Object.values(material).forEach((value) => {
    const texture = value as Texture;
    if (!texture?.isTexture || disposed.has(texture)) return;
    disposed.add(texture);
    texture.dispose();
  });
}
