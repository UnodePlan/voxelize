import { Inputs, RigidControls, VoxelInteract, World } from "@voxelize/core";
import {
  AmbientLight,
  Color,
  DirectionalLight,
  NoToneMapping,
  PerspectiveCamera,
  SRGBColorSpace,
  Vector3,
  WebGLRenderer,
  type Group,
} from "three";

import { disposeObjectTree } from "../game/object-disposal";

import { LocalAnimalSystem, planBiomeAnimals } from "./animals";
import { createExtractionBeacon } from "./beacon";
import { BlockBreakFx } from "./block-break-fx";
import type { LocalHeldContent } from "./held-content";
import {
  createEmptyMcSceneActors,
  disposeMcSceneActors,
  spawnMcSceneActors,
  updateMcSceneActors,
  type McSceneActors,
} from "./mc-scene-actors";
import {
  applyMannequinKnockbackImpulse,
  applyPlayerKnockbackImpulse,
  killMannequinActor,
  mannequinEyePosition,
  playMannequinHurt,
  raycastMannequinActor,
  respawnMannequinNearPoint,
} from "./runtime-mannequin";
import { SelfHeldVisual } from "./self-held";
import { configureLocalAtmosphere } from "./sky-paint";
import { LocalViewmodel, type LocalHeldTool } from "./viewmodel";
import { LocalWorldAdapter } from "./world-adapter";
import { canPlaceLocalBlock, placeLocalBlock } from "./world-place";

export interface LocalRuntimeFrame {
  deltaMs: number;
  /** 本帧有位移输入（WASD） */
  moving: boolean;
  /**
   * RigidControls.state.running（builder 脚步门控用这个，不是任意 movements）
   */
  running: boolean;
  /** 贴地（body.atRestY === -1，与 Lab 一致） */
  onGround: boolean;
  /** Shift 冲刺中 */
  sprinting: boolean;
  playerPosition: Vector3;
  ready: boolean;
  target: { id: number; voxel: [number, number, number] } | null;
  /**
   * 准星邻格放置位（VoxelInteract.potential）。
   * 对齐 examples 右键放置。
   */
  potential: [number, number, number] | null;
}

export interface LocalRuntimeActions {
  onError(message: string): void;
  onPointerLock(locked: boolean): void;
  onWorldReady(): void;
}

/** create.town Peers.MODEL_SCALE = 3.8；bodyHeight 系数来自解包。 */
const CREATE_TOWN_BODY_HEIGHT = 0.434_210_526 * 3.8;

export class LocalWorldRuntime {
  readonly adapter = new LocalWorldAdapter();
  // create.town/lab：PerspectiveCamera(90, ...)
  readonly camera = new PerspectiveCamera(90, 1, 0.1, 3_000);
  readonly world = new World({
    clientOnlyMeshing: true,
    // 与 lab 云层噪声参数对齐（解包 cloudsOptions）
    cloudsOptions: {
      height: 7,
      cloudHeight: 300,
      noiseScale: 0.04,
      threshold: 0.09,
      dimensions: [24, 12, 24],
      alpha: 0.6,
      octaves: 6,
      falloff: 0.85,
    },
    chunkUniformsOverwrite: {
      // lab: lightIntensityAdjustment: 0.75 — 压 AO 棋盘对比
      lightIntensityAdjustment: { value: 0.75 },
    },
    defaultRenderRadius: 6,
    fogFarRenderRatio: 0.78,
    fogNearRenderRatio: 0.45,
    maxLightWorkers: 2,
    maxMeshesPerUpdate: 16,
    maxProcessesPerUpdate: 4,
    maxUpdatesPerUpdate: 10_000,
    mergeChunkGeometries: true,
    minLightLevel: this.adapter.map.style.minLightLevel,
    textureUnitDimension: 16,
    useLightWorkers: true,
  });

  private readonly renderer: WebGLRenderer;
  private readonly controls: RigidControls;
  private readonly inputs = new Inputs<"disabled" | "in-game">();
  private readonly interact: VoxelInteract;
  private readonly disconnectControls: () => void;
  private readonly disconnectArm: () => void;
  private readonly resizeObserver: ResizeObserver;
  private readonly direction = new Vector3();
  private readonly beacon: Group;
  private readonly viewmodel: LocalViewmodel;
  private readonly armCamera: PerspectiveCamera;
  private readonly breakFx: BlockBreakFx;
  private readonly ambientLight: AmbientLight;
  private animals: LocalAnimalSystem | null = null;
  /** 出生点假人 + 第一人称自身身体 */
  private mcActors: McSceneActors = createEmptyMcSceneActors();
  /** 自身第三人称手持（与假人 demo mesh 分离） */
  private readonly selfHeld = new SelfHeldVisual();
  private initialized = false;
  private ready = false;
  private disposed = false;
  private interactive = true;
  private lastFrameAt = 0;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly actions: LocalRuntimeActions,
  ) {
    this.renderer = new WebGLRenderer({
      canvas,
      antialias: false,
      powerPreference: "high-performance",
    });
    this.renderer.outputColorSpace = SRGBColorSpace;
    this.renderer.toneMapping = NoToneMapping;
    this.renderer.autoClear = true;

    this.controls = new RigidControls(this.camera, canvas, this.world, {
      bodyDepth: 0.8,
      bodyHeight: CREATE_TOWN_BODY_HEIGHT,
      bodyWidth: 0.8,
      crouchBodyHeight: CREATE_TOWN_BODY_HEIGHT,
      crouchFactor: 1,
      eyeHeight: 0.919_354_838_709_677_4,
      initialDirection: [...this.adapter.map.initialDirection],
      initialPosition: [0, 40, 27],
      maxSpeed: 5,
      sensitivity: 100,
      sprintFactor: 1.3,
      stepHeight: 0.5,
    });
    this.disconnectControls = this.controls.connect(this.inputs, "in-game");
    this.inputs.setNamespace("disabled");
    this.controls.on("lock", this.handleLock);
    this.controls.on("unlock", this.handleUnlock);

    // lab: outline 黑色 0.5；采集 reach 保持 5（建造才是 32）
    this.interact = new VoxelInteract(this.controls.object, this.world, {
      highlightColor: new Color("#000000"),
      highlightOpacity: 0.5,
      highlightType: "outline",
      ignoreFluids: true,
      inverseDirection: true,
      potentialVisuals: false,
      reachDistance: 5,
    });
    this.world.add(this.interact);
    this.beacon = createExtractionBeacon(this.adapter.map.extraction);
    this.world.add(this.beacon);
    this.breakFx = new BlockBreakFx(this.world);

    this.ambientLight = new AmbientLight(
      new Color(this.adapter.map.style.ambientColor),
      this.adapter.map.style.ambientIntensity,
    );
    this.world.add(this.ambientLight);
    // 体素人模需要方向光才有立体感
    const sun = new DirectionalLight(0xfff2e0, 0.85);
    sun.position.set(40, 80, 20);
    this.world.add(sun);

    // 独立手臂相机：固定在原点朝 -Z，不跟随玩家世界位姿（examples + lab 同构）
    this.armCamera = new PerspectiveCamera(90, 1, 0.05, 10);
    this.viewmodel = new LocalViewmodel(this.armCamera);
    this.controls.attachArm(this.viewmodel.arm);
    // attachArm 默认 emitSwingEvent 会调 this.character.playArmSwingAnimation()；
    // 单机未 attachCharacter，挖掘/左键挥动会抛 TypeError 并打断 rAF，表现为卡死。
    this.viewmodel.arm.emitSwingEvent = () => {
      /* 单机：只播本地手臂动画，不推网络包、不依赖第三人称 Character */
    };
    this.disconnectArm = this.viewmodel.arm.connect(this.inputs, "in-game");

    configureLocalAtmosphere(this.world, this.adapter.map.style);
    this.animals = this.createAnimals();
    void this.spawnMcActors();

    this.resizeObserver = new ResizeObserver(this.resize);
    this.resizeObserver.observe(canvas);
    this.resize();
  }

  get isLocked(): boolean {
    return this.controls.isLocked;
  }

  async initialize(): Promise<void> {
    try {
      await this.adapter.initializeWorld(this.world);
      if (this.disposed) return;
      this.world.renderRadius = 6;
      // 高空开局：清零速度后从 spawn 天空点落下
      this.dropInFromSky(this.adapter.map.spawn);
      this.initialized = true;
      this.canvas.dataset.singleWorld = "loading";
    } catch {
      this.actions.onError("本地体素世界初始化失败");
    }
  }

  update(now: number): LocalRuntimeFrame {
    const deltaMs =
      this.lastFrameAt === 0 ? 0 : Math.min(100, now - this.lastFrameAt);
    this.lastFrameAt = now;
    if (!this.initialized || this.disposed) return this.emptyFrame(deltaMs);

    if (this.ready && this.interactive) {
      this.controls.update();
      this.interact.update();
    }
    const position = this.controls.object.position;
    this.canvas.dataset.singlePosition = position
      .toArray()
      .map((value) => value.toFixed(2))
      .join(",");
    this.camera.getWorldDirection(this.direction);
    this.world.update(position, this.direction);
    this.adapter.drainWorldPackets(this.world);

    if (!this.ready && this.adapter.isWorldReady(this.world)) {
      this.ready = true;
      // 区块就绪后再放一次高空，避免加载期间提前落地
      this.dropInFromSky(this.adapter.map.spawn);
      this.canvas.dataset.singleWorld = "ready";
      this.actions.onWorldReady();
    }

    this.breakFx.update(deltaMs);
    if (this.ready) {
      this.animals?.update(deltaMs);
    }

    // builder 脚步只用 state.running；moving 另含其它输入，供 UI/逻辑
    const running = this.ready && this.controls.state.running;
    const moving =
      running ||
      (this.ready && Object.values(this.controls.movements).some(Boolean));
    // Lab AudioProvider：running && atRestY === -1 && !swimming
    const onGround = this.ready && this.controls.body.atRestY === -1;
    const sprinting = this.ready && this.controls.state.sprinting;

    // 假人巡逻 + 第一人称自身（眼位/低头门控）
    updateMcSceneActors(this.mcActors, deltaMs, {
      runDemo: this.ready,
      selfFrame: { position, lookDir: this.direction, moving },
    });

    this.world.updateShaderLighting(this.camera, position);

    // create.town / examples 双 pass：世界 → clearDepth → 手臂场景
    this.renderer.autoClear = true;
    this.renderer.render(this.world, this.camera);
    this.renderer.autoClear = false;
    this.renderer.clearDepth();
    // armCamera：只同步 aspect；near/far/fov 固定，避免主相机 far=3000 等干扰手持裁剪
    this.armCamera.aspect = this.camera.aspect;
    this.armCamera.fov = 90;
    this.armCamera.near = 0.05;
    this.armCamera.far = 10;
    this.armCamera.updateProjectionMatrix();
    this.renderer.render(this.viewmodel.scene, this.armCamera);
    this.renderer.autoClear = true;

    const targetVoxel =
      this.ready && this.interactive ? this.interact.target : null;
    const potentialVoxel =
      this.ready && this.interactive
        ? this.interact.potential?.voxel ?? null
        : null;
    return {
      deltaMs,
      moving,
      running,
      onGround,
      sprinting,
      playerPosition: position.clone(),
      ready: this.ready,
      target:
        targetVoxel === null
          ? null
          : {
              id: this.world.getVoxelAt(...targetVoxel),
              voxel: [...targetVoxel],
            },
      potential:
        potentialVoxel === null
          ? null
          : ([...potentialVoxel] as [number, number, number]),
    };
  }

  setMiningProgress(progress: number | null): void {
    this.viewmodel.setMiningProgress(progress);
  }

  /**
   * MC 裂纹进度：progress 0–1 映射 destroy_stage 0–9；null 隐藏。
   * 单目标兼容 API；多块常驻裂纹请用 setBreakCracks。
   */
  setBreakCrack(
    progress: number | null,
    voxel: readonly [number, number, number] | null,
  ): void {
    this.breakFx.setCrack(progress, voxel);
  }

  /** 同步全部已损伤方块裂纹（不依赖准星，可多块同时显示） */
  setBreakCracks(
    entries: ReadonlyArray<{
      key: string;
      progress: number;
      voxel: readonly [number, number, number];
    }>,
  ): void {
    this.breakFx.setCracks(entries);
  }

  /** 方块打碎碎片喷发 */
  playBlockBreakBurst(
    voxel: readonly [number, number, number],
    color: string,
    now: number,
  ): void {
    this.breakFx.burst(voxel, color, now);
  }

  /**
   * 同步握持：第一人称 viewmodel + 第三人称自身右臂。
   * 支持空手 / 镐剑 / 背包方块。
   */
  setHeldContent(content: LocalHeldContent, animate = true): void {
    this.viewmodel.setHeldContent(content, animate);
    void this.selfHeld.sync(content, this.mcActors.selfBody, () =>
      this.viewmodel.heldContent,
    );
  }

  /** @deprecated 兼容仅工具三态 */
  setHeldTool(tool: LocalHeldTool, animate = true): void {
    if (tool === "empty") {
      this.setHeldContent({ kind: "empty" }, animate);
    } else {
      this.setHeldContent({ kind: "tool", tool }, animate);
    }
  }

  get heldTool(): LocalHeldTool {
    return this.viewmodel.heldTool;
  }

  get heldContent(): LocalHeldContent {
    return this.viewmodel.heldContent;
  }

  /**
   * 调试：第一人称 Arm 子树（是否有右手/方块）+ 世界坐标。
   */
  debugArmHeld(): {
    held: LocalHeldContent;
    childCount: number;
    children: Array<{
      name: string;
      type: string;
      childCount: number;
      worldPos: [number, number, number];
      visible: boolean;
    }>;
  } {
    const arm = this.viewmodel.arm;
    const children: Array<{
      name: string;
      type: string;
      childCount: number;
      worldPos: [number, number, number];
      visible: boolean;
    }> = [];
    arm.updateMatrixWorld(true);
    arm.traverse((obj) => {
      if (obj === arm) return;
      const p = new Vector3();
      obj.getWorldPosition(p);
      children.push({
        name: obj.name || obj.type,
        type: obj.type,
        childCount: obj.children.length,
        worldPos: [p.x, p.y, p.z],
        visible: obj.visible,
      });
    });
    return {
      held: this.viewmodel.heldContent,
      childCount: arm.children.length,
      children: children.slice(0, 24),
    };
  }

  getDirection(): Vector3 {
    return this.camera.getWorldDirection(new Vector3()).normalize();
  }

  raycastMannequin(maxDistance: number): number | null {
    return raycastMannequinActor(
      this.mcActors,
      this.controls.object.position,
      this.getDirection(),
      maxDistance,
    );
  }

  playMannequinHurtFeedback(): void {
    playMannequinHurt(this.mcActors);
  }

  applyMannequinKnockback(impulse: readonly [number, number, number]): boolean {
    return applyMannequinKnockbackImpulse(this.mcActors, impulse);
  }

  killMannequin(): "sword" | "pickaxe" | null {
    return killMannequinActor(this.mcActors);
  }

  getMannequinEyePosition(): [number, number, number] | null {
    return mannequinEyePosition(this.mcActors);
  }

  respawnMannequinNear(
    near: readonly [number, number],
    pathHalfLength = 2.5,
  ): void {
    respawnMannequinNearPoint(this.mcActors, near, pathHalfLength);
  }

  applyPlayerKnockback(impulse: readonly [number, number, number]): void {
    applyPlayerKnockbackImpulse(this.controls, impulse);
  }

  playAttackSwing(): void {
    this.viewmodel.arm.doSwing();
  }

  /** 是否可在 potential 邻格放置（见 world-place） */
  canPlaceBlock(
    voxel: readonly [number, number, number],
    blockId: number,
  ): boolean {
    return canPlaceLocalBlock(this.placeContext(), voxel, blockId);
  }

  /** 本地放置方块（server 源立即生效） */
  placeBlock(
    voxel: readonly [number, number, number],
    blockId: number,
  ): boolean {
    return placeLocalBlock(this.placeContext(), voxel, blockId);
  }

  private placeContext() {
    return {
      ready: this.ready,
      disposed: this.disposed,
      world: this.world,
      controls: this.controls,
      adapter: this.adapter,
    };
  }

  /**
   * 坠落出图后：传送到地图内随机格上方高空，带零速度落下。
   * @returns 落点（眼睛坐标）
   */
  respawnRandomFromSky(
    drop: readonly [number, number, number],
  ): readonly [number, number, number] {
    this.dropInFromSky(drop);
    return drop;
  }

  /** 清零移动后放到高空点；teleportToExact 会清零速度，由重力下落 */
  private dropInFromSky(eye: readonly [number, number, number]): void {
    this.controls.resetMovements();
    this.controls.teleportToExact(eye[0], eye[1], eye[2]);
    this.controls.setDirection(...this.adapter.map.initialDirection);
  }

  freeze(): void {
    this.interactive = false;
    this.interact.toggle(false);
    this.inputs.setNamespace("disabled");
    this.controls.resetMovements();
    if (this.controls.isLocked) this.controls.unlock();
  }

  /** 打开背包等 UI 时释放指针锁，回到可点选界面。 */
  unlockPointer(): void {
    this.controls.resetMovements();
    if (this.controls.isLocked) this.controls.unlock();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.resizeObserver.disconnect();
    this.controls.off("lock", this.handleLock);
    this.controls.off("unlock", this.handleUnlock);
    this.disconnectArm();
    this.disconnectControls();
    this.inputs.reset();
    if (document.pointerLockElement === this.canvas) document.exitPointerLock();
    disposeObjectTree(this.interact, true);
    disposeObjectTree(this.beacon, true);
    disposeMcSceneActors(this.mcActors, (root) => {
      this.world.remove(root);
    });
    this.mcActors = createEmptyMcSceneActors();
    this.selfHeld.dispose();
    this.animals?.dispose();
    this.animals = null;
    this.breakFx.dispose();
    this.viewmodel.dispose();
    this.world.dispose();
    this.renderer.dispose();
  }

  private readonly handleLock = (): void => {
    if (!this.interactive) return;
    this.inputs.setNamespace("in-game");
    this.actions.onPointerLock(true);
  };

  private readonly handleUnlock = (): void => {
    this.inputs.setNamespace("disabled");
    this.controls.resetMovements();
    this.actions.onPointerLock(false);
  };

  private readonly resize = (): void => {
    const width = Math.max(1, this.canvas.clientWidth);
    const height = Math.max(1, this.canvas.clientHeight);
    // lab: setPixelRatio(window.devicePixelRatio || 1)
    this.renderer.setPixelRatio(window.devicePixelRatio || 1);
    this.renderer.setSize(width, height, false);
    this.camera.aspect = width / height;
    this.camera.updateProjectionMatrix();
    this.viewmodel.resize(width, height);
  };

  private createAnimals(): LocalAnimalSystem {
    const map = this.adapter.map;
    const plans = planBiomeAnimals(map.seed, map.style.biome, map.surfaceY);
    return new LocalAnimalSystem(
      this.world,
      plans,
      map.surfaceY,
      () => false,
      map.seed,
    );
  }

  private async spawnMcActors(): Promise<void> {
    const eyeFromFeet =
      this.controls.options.bodyHeight * this.controls.options.eyeHeight;
    const actors = await spawnMcSceneActors({
      isDisposed: () => this.disposed,
      addToWorld: (root) => {
        this.world.add(root);
      },
      surfaceY: this.adapter.map.surfaceY,
      eyeFromFeet,
    });
    if (this.disposed) {
      disposeMcSceneActors(actors, (root) => {
        this.world.remove(root);
      });
      return;
    }
    this.mcActors = actors;
    // 身体异步就绪后补挂当前手持（可能已选中资源槽）
    this.selfHeld.invalidateAttachment();
    void this.selfHeld.sync(
      this.viewmodel.heldContent,
      this.mcActors.selfBody,
      () => this.viewmodel.heldContent,
    );
  }

  private emptyFrame(deltaMs: number): LocalRuntimeFrame {
    return {
      deltaMs,
      moving: false,
      running: false,
      onGround: false,
      sprinting: false,
      playerPosition: this.controls.object.position.clone(),
      ready: false,
      target: null,
      potential: null,
    };
  }
}
