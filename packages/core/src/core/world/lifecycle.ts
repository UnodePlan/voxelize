import type {
  BufferGeometry,
  Material,
  Mesh,
  Object3D,
  Scene,
  Texture,
} from "three";

import type { ChunkRenderer } from "./chunk-renderer";
import type { Clouds } from "./clouds";
import type { CSMRenderer } from "./csm-renderer";
import type { ItemRegistry } from "./items";
import type { Loader } from "./loader";
import type { ChunkPipeline } from "./pipelines";
import type { Registry } from "./registry";
import type { Sky } from "./sky";
import { AtlasTexture } from "./textures";

type DisposalState = {
  geometries: Set<BufferGeometry>;
  materials: Set<Material>;
  textures: Set<Texture>;
};

export type WorldLifecycleResources = {
  scene: Scene;
  chunkPipeline: ChunkPipeline;
  chunkRenderer: ChunkRenderer;
  blockMeshCache: Map<string, Object3D>;
  sky: Sky;
  clouds: Clouds;
  loader: Loader;
  registry: Registry;
  items: ItemRegistry;
  csmRenderer: CSMRenderer | null;
};

function isTexture(value: unknown): value is Texture {
  return Boolean(
    value &&
      typeof value === "object" &&
      (value as { isTexture?: boolean }).isTexture,
  );
}

function disposeTexture(texture: Texture, state: DisposalState): void {
  if (state.textures.has(texture)) return;
  state.textures.add(texture);
  // 未知纹理跨 World 复用，单局退出不能销毁这份进程级资源。
  if (AtlasTexture.isSharedUnknownTexture(texture)) return;
  texture.dispose();
}

function disposeMaterial(material: Material, state: DisposalState): void {
  if (state.materials.has(material)) return;
  state.materials.add(material);

  for (const value of Object.values(material)) {
    if (isTexture(value)) disposeTexture(value, state);
  }

  material.dispose();
}

/** Dispose only resources owned by the given object tree. */
export function disposeObject3DResources(
  root: Object3D,
  state: DisposalState = {
    geometries: new Set(),
    materials: new Set(),
    textures: new Set(),
  },
): void {
  root.traverse((object) => {
    const mesh = object as Mesh;
    if (!mesh.isMesh) return;

    const geometry = mesh.geometry;
    if (geometry && !state.geometries.has(geometry)) {
      state.geometries.add(geometry);
      geometry.dispose();
    }

    const materials = Array.isArray(mesh.material)
      ? mesh.material
      : [mesh.material];
    materials
      .filter(Boolean)
      .forEach((material) => disposeMaterial(material, state));
  });

  root.clear();
}

function disposeChunks(chunkPipeline: ChunkPipeline): void {
  const loadedChunks: Array<{
    name: string;
    group: Object3D;
    dispose: () => void;
  }> = [];

  chunkPipeline.forEachLoaded((chunk, name) => {
    loadedChunks.push({
      name,
      group: chunk.group,
      dispose: () => chunk.dispose(),
    });
  });

  // 先释放实体区块，再清除所有阶段索引，避免外部仍持有旧管线时残留引用。
  for (const chunk of loadedChunks) {
    chunk.group.removeFromParent();
    chunk.dispose();
  }

  for (const stage of ["requested", "processing", "loaded"] as const) {
    for (const name of [...chunkPipeline.getInStage(stage)]) {
      chunkPipeline.remove(name);
    }
  }
}

function disposeChunkRenderer(
  chunkRenderer: ChunkRenderer,
  state: DisposalState,
): void {
  for (const material of chunkRenderer.materials.values()) {
    disposeMaterial(material, state);
  }
  chunkRenderer.materials.clear();

  disposeTexture(chunkRenderer.uniforms.sceneColor.value, state);
  chunkRenderer.shaderLightingUniforms.shadowMap0.value = null;
  chunkRenderer.shaderLightingUniforms.shadowMap1.value = null;
  chunkRenderer.shaderLightingUniforms.shadowMap2.value = null;
  chunkRenderer.uniforms.waterRefractionReady.value = 0;
}

/** Release resources owned by a world while leaving external scene objects undisposed. */
export function disposeWorldLifecycle({
  scene,
  chunkPipeline,
  chunkRenderer,
  blockMeshCache,
  sky,
  clouds,
  loader,
  registry,
  items,
  csmRenderer,
}: WorldLifecycleResources): void {
  const state: DisposalState = {
    geometries: new Set(),
    materials: new Set(),
    textures: new Set(),
  };

  disposeChunks(chunkPipeline);

  for (const mesh of blockMeshCache.values()) {
    disposeObject3DResources(mesh, state);
  }
  blockMeshCache.clear();

  disposeObject3DResources(sky, state);
  sky.boxLayers.forEach((layer) => layer.materials.clear());
  sky.boxLayers.length = 0;
  sky.shadingData.length = 0;
  clouds.dispose();
  disposeChunkRenderer(chunkRenderer, state);
  loader.dispose();
  items.dispose();

  registry.blocksByName.clear();
  registry.blocksById.clear();
  registry.nameMap.clear();
  registry.idMap.clear();

  csmRenderer?.dispose();
  scene.clear();
}
