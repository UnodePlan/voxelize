import {
  Inputs,
  RigidControls,
  VoxelInteract,
  World,
  artFunctions,
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

import { createExtractionBeacon } from "./beacon";
import { LocalViewmodel } from "./viewmodel";
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
    minLightLevel: 0.04,
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

    // lab: AmbientLight(0xffffff, 0.3)
    this.world.add(new AmbientLight(0xffffff, 0.3));

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

    this.configureAtmosphere();

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
      this.controls.teleportToExact(...this.adapter.map.spawn);
      this.controls.setDirection(...this.adapter.map.initialDirection);
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
      this.controls.teleportToExact(...this.adapter.map.spawn);
      this.controls.setDirection(...this.adapter.map.initialDirection);
      this.canvas.dataset.singleWorld = "ready";
      this.actions.onWorldReady();
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
    this.controls.resetMovements();
    this.controls.teleportToExact(drop[0], drop[1], drop[2]);
    return drop;
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

  private configureAtmosphere(): void {
    // lab: 完整昼夜 phases + 多层天空绘制（layers 在 Sky 构造后通过 paint 分层表达）
    this.world.sky.visible = true;
    this.world.clouds.visible = true;
    this.world.sky.setShadingPhases([
      {
        name: "sunrise",
        start: 0.2,
        color: { top: "#5D6DB5", middle: "#FF9A6C", bottom: "#FFC670" },
        skyOffset: 0.15,
        voidOffset: 0.6,
      },
      {
        name: "daylight",
        start: 0.25,
        color: { top: "#4A90D9", middle: "#7EC8E3", bottom: "#C9E4F6" },
        skyOffset: 0.2,
        voidOffset: 0.6,
      },
      {
        name: "sunset",
        start: 0.65,
        color: { top: "#4A5F8C", middle: "#FF7B54", bottom: "#FFB26B" },
        skyOffset: 0.15,
        voidOffset: 0.6,
      },
      {
        name: "twilight",
        start: 0.75,
        color: { top: "#1A1A2E", middle: "#2A2040", bottom: "#3A3050" },
        skyOffset: 0.08,
        voidOffset: 0.6,
      },
      {
        name: "night",
        start: 0.88,
        color: { top: "#010101", middle: "#000000", bottom: "#000000" },
        skyOffset: 0.1,
        voidOffset: 0.6,
      },
    ]);
    // 与 lab 类似：底面太阳 + 顶面星空（原创绘制，不用其贴图）
    this.world.sky.paint("bottom", (context, canvas) => {
      paintCreateTownStyleSun(context, canvas);
    });
    this.world.sky.paint("top", artFunctions.drawStars());
    this.world.sky.paint("sides", artFunctions.drawStars());
    this.world.background = new Color("#4A90D9");
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
