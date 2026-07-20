import type { MessageProtocol } from "@voxelize/protocol";
import {
  AmbientLight,
  BoxGeometry,
  Color,
  DirectionalLight,
  Fog,
  Group,
  HemisphereLight,
  InstancedMesh,
  MathUtils,
  Matrix4,
  Mesh,
  MeshStandardMaterial,
  PerspectiveCamera,
  RingGeometry,
  Scene,
  SRGBColorSpace,
  WebGLRenderer,
} from "three";

import type { ExtractionManifest } from "../../../../contracts/extraction/v1/typescript";

import type { WorldInputActions } from "./world-input";
import { VoxelWorldSession } from "./world-session";

export type SceneMode = "lobby" | "match" | "result";

export interface VoxelSceneActions extends WorldInputActions {
  getManifest(): ExtractionManifest | null;
  onError(message: string): void;
  onWorldReady(): void;
  sendWorldPacket(message: MessageProtocol): void;
}

const BLOCK_SIZE = 1;
const GRID_RADIUS = 12;
const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

/** 首页背景：浅空 + 分层采石场；镜头环绕而非小幅抖动 */
const LOBBY_SKY = 0xe8eef2;
const LOBBY_FOG = 0xdde5ea;
const LOBBY_ORBIT_RADIUS = 24;
const LOBBY_ORBIT_HEIGHT = 13;
/** ~28s 一圈，2 秒内约 26°，镜头推进感更明显 */
const LOBBY_ORBIT_SPEED = 0.22;
const LOBBY_LOOK_AT_Y = 0.8;

export class VoxelBackdrop {
  private readonly renderer: WebGLRenderer;
  private readonly scene = new Scene();
  private readonly camera = new PerspectiveCamera(48, 1, 0.1, 160);
  private readonly quarry = new Group();
  private readonly beacon: Mesh;
  private readonly resizeObserver: ResizeObserver;
  private readonly reducedMotion = window.matchMedia(REDUCED_MOTION_QUERY);
  private frame = 0;
  private animationFrame = 0;
  private mode: SceneMode = "lobby";
  private pointerX = 0;
  private pointerY = 0;
  private smoothPointerX = 0;
  private smoothPointerY = 0;
  private orbitAngle = Math.PI * 0.22;
  private liveWorld: VoxelWorldSession | null = null;
  private lastFrameMs = performance.now();

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly actions: VoxelSceneActions,
  ) {
    this.renderer = new WebGLRenderer({
      canvas,
      antialias: true,
      powerPreference: "high-performance",
      alpha: false,
    });
    this.renderer.outputColorSpace = SRGBColorSpace;
    this.applyLobbyEnvironment();
    this.camera.position.set(LOBBY_ORBIT_RADIUS * 0.72, LOBBY_ORBIT_HEIGHT, LOBBY_ORBIT_RADIUS * 0.7);
    this.camera.lookAt(0, LOBBY_LOOK_AT_Y, 0);

    this.scene.add(new HemisphereLight(0xf4f7fa, 0xb8a892, 0.95));
    this.scene.add(new AmbientLight(0xffffff, 0.42));
    const keyLight = new DirectionalLight(0xfff2dd, 1.85);
    keyLight.position.set(-12, 22, 8);
    this.scene.add(keyLight);
    const fill = new DirectionalLight(0xd6e4f0, 0.55);
    fill.position.set(10, 8, -12);
    this.scene.add(fill);

    this.scene.add(this.quarry);
    this.createTerrain();
    this.beacon = this.createBeacon();
    this.quarry.add(this.beacon);

    this.resizeObserver = new ResizeObserver(() => this.resize());
    this.resizeObserver.observe(canvas);
    window.addEventListener("pointermove", this.handlePointer, {
      passive: true,
    });
    this.resize();
    this.animate();
  }

  setMode(mode: SceneMode): void {
    this.mode = mode;
    this.canvas.dataset.sceneMode = mode;
    // 撤离信标仅在结果相关氛围露出；大厅隐藏以免抢戏
    this.beacon.visible = mode === "result";
  }

  handleNetworkMessage(message: MessageProtocol): void {
    if (message.type === "INIT") {
      const manifest = this.actions.getManifest();
      if (manifest === null) {
        this.actions.onError("资源清单尚未就绪");
        return;
      }
      this.disposeLiveWorld();
      this.liveWorld = new VoxelWorldSession(
        this.canvas,
        manifest,
        this.actions,
      );
      this.liveWorld.resize(
        Math.max(1, this.canvas.clientWidth),
        Math.max(1, this.canvas.clientHeight),
      );
    }
    this.liveWorld?.enqueue(message);
  }

  resetLiveWorld(): void {
    this.disposeLiveWorld();
  }

  dispose(): void {
    cancelAnimationFrame(this.animationFrame);
    this.resizeObserver.disconnect();
    window.removeEventListener("pointermove", this.handlePointer);
    this.disposeLiveWorld();
    this.renderer.dispose();
  }

  private applyLobbyEnvironment(): void {
    this.scene.background = new Color(LOBBY_SKY);
    this.scene.fog = new Fog(LOBBY_FOG, 28, 72);
  }

  private createTerrain(): void {
    const geometry = new BoxGeometry(BLOCK_SIZE, BLOCK_SIZE, BLOCK_SIZE);
    // 单材质 + instanceColor 分层上色（表土 / 岩 / 深层）
    const rockMaterial = new MeshStandardMaterial({
      color: 0x8a8276,
      roughness: 0.9,
      metalness: 0.02,
    });

    const count = (GRID_RADIUS * 2 + 1) ** 2;
    const blocks = new InstancedMesh(geometry, rockMaterial, count);
    const matrix = new Matrix4();
    const color = new Color();
    let index = 0;

    for (let x = -GRID_RADIUS; x <= GRID_RADIUS; x += 1) {
      for (let z = -GRID_RADIUS; z <= GRID_RADIUS; z += 1) {
        const distance = Math.hypot(x, z);
        // 深坑 + 高台 + 断崖：远看更有「遗址采石场」剪影
        const pit = Math.max(0, 4.5 - distance);
        const terrace = distance > 7 ? Math.floor((distance - 7) * 0.9) : 0;
        const ridge =
          Math.abs(x) === GRID_RADIUS || Math.abs(z) === GRID_RADIUS ? 2 : 0;
        const noise =
          Math.sin(x * 0.48 + z * 0.27) * 0.9 +
          Math.cos(x * 0.17 - z * 0.41) * 0.5;
        // 局部石柱 / 断墙
        const pillar =
          (Math.abs(x) === 6 && Math.abs(z) <= 1) ||
          (Math.abs(z) === 5 && x >= 2 && x <= 4)
            ? 3 + (Math.abs(x + z) % 2)
            : 0;
        const y = MathUtils.clamp(
          Math.round(-pit * 0.85 + terrace + ridge + noise * 0.45 + pillar - 0.5),
          -4,
          8,
        );
        matrix.makeTranslation(x * BLOCK_SIZE, y * BLOCK_SIZE, z * BLOCK_SIZE);
        blocks.setMatrixAt(index, matrix);

        if (pillar > 0) {
          color.setHex(0x9a9488);
        } else if (y >= 3) {
          color.setHex(0xb4a890);
        } else if (y >= 1) {
          color.setHex(0x978f7e);
        } else if (y >= -1) {
          color.setHex(0x7a7468);
        } else {
          color.setHex(0x555c62);
        }
        const checker = ((x + z + 40) % 3) * 0.018;
        color.offsetHSL(0, 0, checker - 0.015);
        blocks.setColorAt(index, color);
        index += 1;
      }
    }
    blocks.instanceMatrix.needsUpdate = true;
    if (blocks.instanceColor !== null) {
      blocks.instanceColor.needsUpdate = true;
    }
    this.quarry.add(blocks);

    // 资源矿脉：黄金偏暖、钻石偏冷青，体量更大一点便于远景识别
    this.createResourceVein(geometry, 0xe0b43a, [5, 0, -3], 10);
    this.createResourceVein(geometry, 0x3ec9c4, [-4, -1, 2], 8);
    // 少量泥土高台，呼应可挖资源叙事
    this.createResourceVein(geometry, 0x8b6f4e, [-1, 1, -5], 4);
  }

  private createResourceVein(
    geometry: BoxGeometry,
    color: number,
    origin: [number, number, number],
    count: number,
  ): void {
    const material = new MeshStandardMaterial({
      color,
      roughness: 0.48,
      metalness: 0.22,
      emissive: new Color(color).multiplyScalar(0.08),
      emissiveIntensity: 0.35,
    });
    const vein = new InstancedMesh(geometry, material, count);
    const matrix = new Matrix4();
    for (let index = 0; index < count; index += 1) {
      const x = origin[0] + (index % 3) - 1;
      const y = origin[1] + (Math.floor(index / 3) % 2);
      const z = origin[2] + (Math.floor(index / 2) % 2);
      matrix.makeTranslation(x, y, z);
      vein.setMatrixAt(index, matrix);
    }
    vein.instanceMatrix.needsUpdate = true;
    this.quarry.add(vein);
  }

  private createBeacon(): Mesh {
    const geometry = new RingGeometry(2.4, 2.7, 48);
    const material = new MeshStandardMaterial({
      color: 0x1a1a1a,
      emissive: 0x222222,
      emissiveIntensity: 0.4,
      roughness: 0.4,
      side: 2,
    });
    const ring = new Mesh(geometry, material);
    ring.rotation.x = -Math.PI / 2;
    ring.position.set(0, -1.2, 0);
    ring.visible = false;
    return ring;
  }

  private readonly handlePointer = (event: PointerEvent): void => {
    this.pointerX = event.clientX / Math.max(1, window.innerWidth) - 0.5;
    this.pointerY = event.clientY / Math.max(1, window.innerHeight) - 0.5;
  };

  private resize(): void {
    const width = Math.max(1, this.canvas.clientWidth);
    const height = Math.max(1, this.canvas.clientHeight);
    const pixelRatio = Math.min(window.devicePixelRatio, 1.5);
    this.renderer.setPixelRatio(pixelRatio);
    this.renderer.setSize(width, height, false);
    this.camera.aspect = width / height;
    this.camera.updateProjectionMatrix();
    this.liveWorld?.resize(width, height);
  }

  private animate = (): void => {
    if (this.mode === "match" && this.liveWorld?.ready === true) {
      this.liveWorld.update();
      this.liveWorld.render(this.renderer);
      this.recordFrame();
      this.animationFrame = requestAnimationFrame(this.animate);
      return;
    }

    const now = performance.now();
    const dt = Math.min(0.05, (now - this.lastFrameMs) / 1000);
    this.lastFrameMs = now;

    // 指针视差平滑，避免硬跟手
    this.smoothPointerX = MathUtils.damp(
      this.smoothPointerX,
      this.pointerX,
      4,
      dt,
    );
    this.smoothPointerY = MathUtils.damp(
      this.smoothPointerY,
      this.pointerY,
      4,
      dt,
    );

    const reduced = this.reducedMotion.matches;
    if (!reduced) {
      this.orbitAngle += LOBBY_ORBIT_SPEED * dt;
    }

    // 环绕 + 呼吸式推近拉远 + 高度起伏；构图略偏左给右侧白侧栏让位
    const compositionBias = -0.42;
    const angle = this.orbitAngle + this.smoothPointerX * 0.28 + compositionBias;
    const heightBob = reduced
      ? 0
      : Math.sin(now * 0.00055) * 1.1 + this.smoothPointerY * -1.1;
    const pulse = reduced ? 0 : Math.sin(now * 0.00028) * 1.6;
    const radius = LOBBY_ORBIT_RADIUS + pulse + this.smoothPointerY * 1.4;

    const targetX = Math.sin(angle) * radius;
    const targetZ = Math.cos(angle) * radius;
    const targetY = LOBBY_ORBIT_HEIGHT + heightBob;

    const follow = 1 - Math.exp(-3.2 * dt);
    this.camera.position.x += (targetX - this.camera.position.x) * follow;
    this.camera.position.y += (targetY - this.camera.position.y) * follow;
    this.camera.position.z += (targetZ - this.camera.position.z) * follow;

    const lookX = this.smoothPointerX * 0.85;
    const lookY = LOBBY_LOOK_AT_Y + this.smoothPointerY * -0.35;
    this.camera.lookAt(lookX, lookY, 0);

    if (!reduced && this.beacon.visible) {
      this.beacon.rotation.z += dt * 0.35;
    }

    this.renderer.render(this.scene, this.camera);
    this.recordFrame();
    this.animationFrame = requestAnimationFrame(this.animate);
  };

  private recordFrame(): void {
    this.frame += 1;
    if (this.frame % 30 === 0) {
      this.canvas.dataset.renderFrame = String(this.frame);
    }
  }

  private disposeLiveWorld(): void {
    const liveWorld = this.liveWorld;
    this.liveWorld = null;
    delete this.canvas.dataset.liveWorld;
    liveWorld?.dispose();
  }
}
