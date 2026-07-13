import { EventEmitter } from "events";

import {
  BoxGeometry,
  FramebufferTexture,
  Group,
  Mesh,
  MeshBasicMaterial,
  Scene,
  Texture,
} from "three";
import { describe, expect, it, vi } from "vitest";

import { World } from ".";
import type { ChunkRenderer } from "./chunk-renderer";
import type { Clouds } from "./clouds";
import { CSMRenderer } from "./csm-renderer";
import type { ItemRegistry } from "./items";
import { disposeWorldLifecycle } from "./lifecycle";
import type { Loader } from "./loader";
import type { ChunkPipeline } from "./pipelines";
import { Registry } from "./registry";
import type { Sky } from "./sky";

vi.mock("../../libs/cull", () => ({ cull: vi.fn() }));

function makePipeline(chunkGroup: Group) {
  const stages = {
    requested: new Set(["requested"]),
    processing: new Set(["processing"]),
    loaded: new Set(["loaded"]),
  };
  const chunkDispose = vi.fn();
  const remove = vi.fn((name: string) => {
    Object.values(stages).forEach((names) => names.delete(name));
  });
  const pipeline = {
    forEachLoaded: (
      callback: (
        chunk: { group: Group; dispose: () => void },
        name: string,
      ) => void,
    ) => callback({ group: chunkGroup, dispose: chunkDispose }, "loaded"),
    getInStage: (stage: keyof typeof stages) => stages[stage],
    remove,
  } as unknown as ChunkPipeline;

  return { pipeline, stages, chunkDispose, remove };
}

function makeOwnedMesh() {
  const texture = new Texture();
  const material = new MeshBasicMaterial({ map: texture });
  const geometry = new BoxGeometry();
  const mesh = new Mesh(geometry, material);

  return {
    mesh,
    disposeGeometry: vi.spyOn(geometry, "dispose"),
    disposeMaterial: vi.spyOn(material, "dispose"),
    disposeTexture: vi.spyOn(texture, "dispose"),
  };
}

describe("disposeWorldLifecycle", () => {
  it("releases owned resources without disposing external scene objects", () => {
    const scene = new Scene();
    const chunkGroup = new Group();
    scene.add(chunkGroup);
    const { pipeline, stages, chunkDispose, remove } = makePipeline(chunkGroup);

    const sky = Object.assign(new Group(), {
      boxLayers: [],
      shadingData: [],
    }) as unknown as Sky;
    const skyMesh = makeOwnedMesh();
    sky.add(skyMesh.mesh);
    scene.add(sky);

    const cachedGroup = new Group();
    const cachedMesh = makeOwnedMesh();
    cachedGroup.add(cachedMesh.mesh);
    const blockMeshCache = new Map([["cached", cachedGroup]]);

    const chunkTexture = new Texture();
    const chunkMaterial = new MeshBasicMaterial({
      map: chunkTexture,
    }) as unknown as ChunkRenderer["materials"] extends Map<string, infer T>
      ? T
      : never;
    const sceneColor = new FramebufferTexture(1, 1);
    const disposeChunkMaterial = vi.spyOn(chunkMaterial, "dispose");
    const disposeChunkTexture = vi.spyOn(chunkTexture, "dispose");
    const disposeSceneColor = vi.spyOn(sceneColor, "dispose");
    const chunkRenderer = {
      materials: new Map([["shared", chunkMaterial]]),
      uniforms: {
        sceneColor: { value: sceneColor },
        waterRefractionReady: { value: 1 },
      },
      shaderLightingUniforms: {
        shadowMap0: { value: new Texture() },
        shadowMap1: { value: new Texture() },
        shadowMap2: { value: new Texture() },
      },
    } as unknown as ChunkRenderer;

    const loader = { dispose: vi.fn() } as unknown as Loader;
    const items = { dispose: vi.fn() } as unknown as ItemRegistry;
    const clouds = { dispose: vi.fn() } as unknown as Clouds;
    const csmRenderer = { dispose: vi.fn() } as unknown as CSMRenderer;
    const registry = new Registry();
    registry.nameMap.set("dirt", 1);

    const external = makeOwnedMesh();
    scene.add(external.mesh);

    disposeWorldLifecycle({
      scene,
      chunkPipeline: pipeline,
      chunkRenderer,
      blockMeshCache,
      sky,
      clouds,
      loader,
      registry,
      items,
      csmRenderer,
    });

    expect(chunkDispose).toHaveBeenCalledOnce();
    expect(remove).toHaveBeenCalledTimes(3);
    expect(Object.values(stages).every((names) => names.size === 0)).toBe(true);
    expect(chunkGroup.parent).toBeNull();

    expect(skyMesh.disposeGeometry).toHaveBeenCalledOnce();
    expect(skyMesh.disposeMaterial).toHaveBeenCalledOnce();
    expect(skyMesh.disposeTexture).toHaveBeenCalledOnce();
    expect(cachedMesh.disposeGeometry).toHaveBeenCalledOnce();
    expect(blockMeshCache.size).toBe(0);

    expect(disposeChunkMaterial).toHaveBeenCalledOnce();
    expect(disposeChunkTexture).toHaveBeenCalledOnce();
    expect(disposeSceneColor).toHaveBeenCalledOnce();
    expect(chunkRenderer.materials.size).toBe(0);
    expect(chunkRenderer.shaderLightingUniforms.shadowMap0.value).toBeNull();

    expect(loader.dispose).toHaveBeenCalledOnce();
    expect(items.dispose).toHaveBeenCalledOnce();
    expect(clouds.dispose).toHaveBeenCalledOnce();
    expect(csmRenderer.dispose).toHaveBeenCalledOnce();
    expect(registry.nameMap.size).toBe(0);
    expect(scene.children).toHaveLength(0);
    expect(external.disposeGeometry).not.toHaveBeenCalled();
    expect(external.disposeMaterial).not.toHaveBeenCalled();
    expect(external.disposeTexture).not.toHaveBeenCalled();
  });

  it("allows CSM resources to be disposed repeatedly", () => {
    const csmRenderer = new CSMRenderer({ cascades: 1, shadowMapSize: 1 });

    csmRenderer.dispose();
    csmRenderer.dispose();

    expect(csmRenderer.numCascades).toBe(0);
  });

  it("makes World disposal idempotent across workers and runtime state", () => {
    const world = new Scene();
    Object.setPrototypeOf(world, World.prototype);

    const stopStats = vi.fn();
    const meshTerminate = vi.fn();
    const urgentMeshTerminate = vi.fn();
    const lightTerminate = vi.fn();
    const resolveLightJobs = vi.fn();
    const chunkGroup = new Group();
    world.add(chunkGroup);
    const { pipeline } = makePipeline(chunkGroup);
    const sceneColor = new FramebufferTexture(1, 1);
    const chunkRenderer = {
      materials: new Map(),
      uniforms: {
        sceneColor: { value: sceneColor },
        waterRefractionReady: { value: 1 },
      },
      shaderLightingUniforms: {
        shadowMap0: { value: null },
        shadowMap1: { value: null },
        shadowMap2: { value: null },
      },
    } as unknown as ChunkRenderer;
    const sky = Object.assign(new Group(), {
      boxLayers: [],
      shadingData: [],
    }) as unknown as Sky;
    const clouds = { dispose: vi.fn() } as unknown as Clouds;
    const loader = { dispose: vi.fn() } as unknown as Loader;
    const items = { dispose: vi.fn() } as unknown as ItemRegistry;

    Object.assign(world, {
      disposed: false,
      isInitialized: true,
      stopStatsSyncInterval: stopStats,
      cleanupDeltasInterval: null,
      meshWorkerPool: { terminate: meshTerminate },
      urgentMeshWorkerPool: { terminate: urgentMeshTerminate },
      lightWorkerPool: { terminate: lightTerminate },
      lightJobsCompleteResolvers: [resolveLightJobs],
      chunkPipeline: pipeline,
      chunkRenderer,
      blockMeshCache: new Map(),
      sky,
      clouds,
      loader,
      registry: new Registry(),
      items,
      csmRenderer: null,
      physics: { bodies: [{}] },
      aabbOverrides: new Map(),
      oldBlocks: new Map(),
      chunkInitializeListeners: new Map(),
      blockEntitiesMap: new Map(),
      blockEntityUpdateListeners: new Set(),
      blockUpdateListeners: new Set(),
      blockEntityFallbackTimeouts: new Set(),
      chunkEvents: new EventEmitter(),
      voxelDeltas: new Map(),
      blockUpdatesQueue: [],
      blockUpdatesToEmit: [],
      lightJobQueue: [],
      packets: [],
      textureLoaderLastMap: {},
      initialData: null,
      initialEntities: null,
      extraInitData: {},
      activeLightBatch: null,
      accumulatedLightOps: null,
      isTrackingChunks: false,
      activeBlockUpdateSource: null,
    });

    const disposableWorld = world as unknown as World;
    disposableWorld.dispose();
    disposableWorld.dispose();

    expect(stopStats).toHaveBeenCalledOnce();
    expect(meshTerminate).toHaveBeenCalledOnce();
    expect(urgentMeshTerminate).toHaveBeenCalledOnce();
    expect(lightTerminate).toHaveBeenCalledOnce();
    expect(resolveLightJobs).toHaveBeenCalledOnce();
    expect(clouds.dispose).toHaveBeenCalledOnce();
    expect(loader.dispose).toHaveBeenCalledOnce();
    expect(items.dispose).toHaveBeenCalledOnce();
    expect(disposableWorld.physics.bodies).toHaveLength(0);
    expect(disposableWorld.isInitialized).toBe(false);
  });

  it("does not revive a disposed world after material loading finishes", async () => {
    const world = new Scene();
    Object.setPrototypeOf(world, World.prototype);
    let finishMaterials!: () => void;
    const materials = new Promise<void>((resolve) => {
      finishMaterials = resolve;
    });
    const postMesh = vi.fn();
    const postUrgentMesh = vi.fn();
    const postLight = vi.fn();
    Object.assign(world, {
      disposed: false,
      isInitialized: false,
      initialData: {
        blocks: {},
        items: [],
        options: {},
        stats: { time: 0 },
      },
      extraInitData: {},
      items: { initialize: vi.fn() },
      registry: new Registry(),
      options: {},
      physics: { options: {} },
      csmRenderer: {},
      loadMaterials: () => materials,
      meshWorkerPool: { postMessage: postMesh },
      urgentMeshWorkerPool: { postMessage: postUrgentMesh },
      lightWorkerPool: { postMessage: postLight },
      initialEntities: null,
    });

    const initializing = (world as unknown as World).initialize();
    await Promise.resolve();
    Object.assign(world, { disposed: true });
    finishMaterials();
    await initializing;

    expect(postMesh).not.toHaveBeenCalled();
    expect(postUrgentMesh).not.toHaveBeenCalled();
    expect(postLight).not.toHaveBeenCalled();
    expect((world as unknown as World).isInitialized).toBe(false);
  });
});
