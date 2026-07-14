import {
  Inputs,
  RigidControls,
  VoxelInteract,
  World,
} from "@voxelize/core";
import {
  AmbientLight,
  Color,
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
import { LocalViewmodel, type LocalHeldTool } from "./viewmodel";
import { LocalWorldAdapter } from "./world-adapter";

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

    this.configureAtmosphere(this.adapter.map.style);
    this.animals = this.createAnimals();

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
    this.world.updateShaderLighting(this.camera, position);

    // create.town / examples 双 pass：世界 → clearDepth → 手臂场景
    this.renderer.autoClear = true;
    this.renderer.render(this.world, this.camera);
    this.renderer.autoClear = false;
    this.renderer.clearDepth();
    // armCamera 只复制投影参数，位姿保持原点（与 examples/create.town 一致）
    this.armCamera.fov = this.camera.fov;
    this.armCamera.near = this.camera.near;
    this.armCamera.far = this.camera.far;
    this.armCamera.aspect = this.camera.aspect;
    this.armCamera.updateProjectionMatrix();
    this.renderer.render(this.viewmodel.scene, this.armCamera);
    this.renderer.autoClear = true;

    const targetVoxel =
      this.ready && this.interactive ? this.interact.target : null;
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
    };
  }

  setMiningProgress(progress: number | null): void {
    this.viewmodel.setMiningProgress(progress);
  }

  /**
   * MC 裂纹进度：progress 0–1 映射 destroy_stage 0–9；null 隐藏。
   */
  setBreakCrack(
    progress: number | null,
    voxel: readonly [number, number, number] | null,
  ): void {
    this.breakFx.setCrack(progress, voxel);
  }

  /** 方块打碎碎片喷发 */
  playBlockBreakBurst(
    voxel: readonly [number, number, number],
    color: string,
    now: number,
  ): void {
    this.breakFx.burst(voxel, color, now);
  }

  /** 同步第一人称握持（空手手臂 / 镐 / 剑） */
  setHeldTool(tool: LocalHeldTool, animate = true): void {
    this.viewmodel.setHeldTool(tool, animate);
  }

  get heldTool(): LocalHeldTool {
    return this.viewmodel.heldTool;
  }

  getDirection(): Vector3 {
    return this.camera.getWorldDirection(new Vector3()).normalize();
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

  private configureAtmosphere(style: {
    backgroundColor: string;
    cloudsVisible: boolean;
    drawSun: boolean;
    drawStars: boolean;
    sky: {
      name: string;
      start: number;
      color: { top: string; middle: string; bottom: string };
      skyOffset: number;
      voidOffset: number;
    };
  }): void {
    this.world.sky.visible = true;
    this.world.clouds.visible = style.cloudsVisible;
    // 单段锁定当前风格时段，避免跨多 phase 混色
    this.world.sky.setShadingPhases([
      {
        name: style.sky.name,
        start: 0,
        color: { ...style.sky.color },
        skyOffset: style.sky.skyOffset,
        voidOffset: style.sky.voidOffset,
      },
      {
        name: `${style.sky.name}-hold`,
        start: 1,
        color: { ...style.sky.color },
        skyOffset: style.sky.skyOffset,
        voidOffset: style.sky.voidOffset,
      },
    ]);
    // 颜色只靠外层 dodecahedron 渐变（setShadingPhases）。
    // 内层 CanvasBox 若铺整面不透明色，会变成「巨大紫色/红色贴纸」盖住渐变
    // （黄昏 top=#3D2A5C、血月底/顶反差都是这个问题）。
    // 内层只允许：透明底 + 星点，和/或底面太阳；绝不整面 fill 实色。
    this.world.sky.paint("all", clearSkyFace);
    if (style.drawStars) {
      this.world.sky.paint("top", paintStarsTransparent());
      this.world.sky.paint("sides", paintStarsTransparent());
      this.world.sky.paint("bottom", paintStarsTransparent());
    }
    if (style.drawSun) {
      this.world.sky.paint("bottom", (context, canvas) => {
        // 保留已有星点（若有），再画太阳辉光
        paintCreateTownStyleSun(context, canvas);
      });
    }
    this.world.background = new Color(style.backgroundColor);
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
    };
  }
}

/** 清空天空盒面为全透明，露出外层渐变 */
function clearSkyFace(
  context: CanvasRenderingContext2D,
  canvas: HTMLCanvasElement,
): void {
  context.save();
  context.setTransform(1, 0, 0, 1, 0, 0);
  context.clearRect(0, 0, canvas.width, canvas.height);
  context.restore();
}

/**
 * 透明底画星：只画点，不铺实色底，渐变天空透过 CanvasBox 可见。
 */
function paintStarsTransparent(
  starCount = 140,
): (context: CanvasRenderingContext2D, canvas: HTMLCanvasElement) => void {
  return (context, canvas) => {
    context.save();
    context.setTransform(1, 0, 0, 1, 0, 0);
    // 不 clear：调用方已 paint("all", clearSkyFace)；此处叠加即可
    const colors = [
      "#FFFFFF",
      "#FFFFFF",
      "#FFE8E0",
      "#FFD0C8",
      "#E8E8FF",
      "#FF8585",
    ];
    for (let i = 0; i < starCount; i += 1) {
      context.globalAlpha = 0.45 + Math.random() * 0.55;
      context.beginPath();
      context.arc(
        Math.random() * canvas.width,
        Math.random() * canvas.height,
        Math.random() * 0.7 + 0.15,
        0,
        Math.PI * 2,
      );
      context.fillStyle = colors[Math.floor(Math.random() * colors.length)];
      context.fill();
    }
    context.restore();
  };
}

/** 按 lab 解包逻辑重绘太阳：低分 canvas + 径向辉光 + 实心核（原创实现）。 */
function paintCreateTownStyleSun(
  context: CanvasRenderingContext2D,
  canvas: HTMLCanvasElement,
): void {
  context.save();
  context.imageSmoothingEnabled = false;
  const low = document.createElement("canvas");
  low.width = Math.max(32, Math.floor(canvas.width / 4));
  low.height = Math.max(32, Math.floor(canvas.height / 4));
  const lowContext = low.getContext("2d");
  if (lowContext === null) {
    context.restore();
    return;
  }
  lowContext.imageSmoothingEnabled = false;
  const cx = low.width / 2;
  const cy = low.height / 2;
  const glow = lowContext.createRadialGradient(cx, cy, 6.25, cx, cy, 31.25);
  glow.addColorStop(0, "rgba(255, 250, 200, 0.4)");
  glow.addColorStop(0.4, "rgba(255, 240, 150, 0.2)");
  glow.addColorStop(1, "rgba(255, 230, 100, 0)");
  lowContext.beginPath();
  lowContext.arc(cx, cy, 31.25, 0, Math.PI * 2);
  lowContext.fillStyle = glow;
  lowContext.fill();
  const core = lowContext.createRadialGradient(cx, cy, 0, cx, cy, 12.5);
  core.addColorStop(0, "rgb(255, 255, 245)");
  core.addColorStop(0.7, "rgb(255, 250, 220)");
  core.addColorStop(1, "rgb(255, 230, 140)");
  lowContext.beginPath();
  lowContext.arc(cx, cy, 12.5, 0, Math.PI * 2);
  lowContext.fillStyle = core;
  lowContext.fill();
  context.drawImage(low, 0, 0, canvas.width, canvas.height);
  context.restore();
}
