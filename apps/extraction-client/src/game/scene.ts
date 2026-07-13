import type { MessageProtocol } from "@voxelize/protocol";
import {
  AmbientLight,
  BoxGeometry,
  Color,
  DirectionalLight,
  Fog,
  Group,
  InstancedMesh,
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
const GRID_RADIUS = 11;
const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

export class VoxelBackdrop {
  private readonly renderer: WebGLRenderer;
  private readonly scene = new Scene();
  private readonly camera = new PerspectiveCamera(58, 1, 0.1, 120);
  private readonly quarry = new Group();
  private readonly beacon: Mesh;
  private readonly resizeObserver: ResizeObserver;
  private readonly reducedMotion = window.matchMedia(REDUCED_MOTION_QUERY);
  private frame = 0;
  private animationFrame = 0;
  private mode: SceneMode = "lobby";
  private pointerX = 0;
  private pointerY = 0;
  private liveWorld: VoxelWorldSession | null = null;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly actions: VoxelSceneActions,
  ) {
    this.renderer = new WebGLRenderer({
      canvas,
      antialias: true,
      powerPreference: "high-performance",
    });
    this.renderer.outputColorSpace = SRGBColorSpace;
    this.scene.background = new Color(0x18211f);
    this.scene.fog = new Fog(0x18211f, 18, 54);
    this.camera.position.set(15, 12, 20);
    this.camera.lookAt(0, 1, 0);

    this.scene.add(new AmbientLight(0xbed1c5, 1.35));
    const keyLight = new DirectionalLight(0xffdfad, 3.1);
    keyLight.position.set(-8, 18, 10);
    this.scene.add(keyLight);

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
    this.beacon.visible = mode !== "lobby";
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

  private createTerrain(): void {
    const geometry = new BoxGeometry(BLOCK_SIZE, BLOCK_SIZE, BLOCK_SIZE);
    const material = new MeshStandardMaterial({
      color: 0x7f7560,
      roughness: 0.94,
      metalness: 0,
    });
    const count = (GRID_RADIUS * 2 + 1) ** 2;
    const blocks = new InstancedMesh(geometry, material, count);
    const matrix = new Matrix4();
    const color = new Color();
    let index = 0;

    for (let x = -GRID_RADIUS; x <= GRID_RADIUS; x += 1) {
      for (let z = -GRID_RADIUS; z <= GRID_RADIUS; z += 1) {
        const distance = Math.hypot(x, z);
        const rim = Math.max(0, Math.floor((distance - 3) / 2));
        const noise = Math.sin(x * 1.7 + z * 0.9) * 0.45;
        const y = Math.max(-2, Math.min(5, rim + Math.round(noise) - 2));
        matrix.makeTranslation(x, y, z);
        blocks.setMatrixAt(index, matrix);
        color.setHSL(0.11, 0.12, 0.31 + ((x + z + 24) % 4) * 0.025);
        blocks.setColorAt(index, color);
        index += 1;
      }
    }
    blocks.instanceMatrix.needsUpdate = true;
    if (blocks.instanceColor !== null) {
      blocks.instanceColor.needsUpdate = true;
    }
    this.quarry.add(blocks);
    this.createResourceVein(geometry, 0xd9ae34, [4, 0, -2], 7);
    this.createResourceVein(geometry, 0x35bfc1, [-3, -1, 1], 5);
  }

  private createResourceVein(
    geometry: BoxGeometry,
    color: number,
    origin: [number, number, number],
    count: number,
  ): void {
    const material = new MeshStandardMaterial({
      color,
      roughness: 0.55,
      metalness: 0.16,
    });
    const vein = new InstancedMesh(geometry, material, count);
    const matrix = new Matrix4();
    for (let index = 0; index < count; index += 1) {
      const x = origin[0] + (index % 3);
      const y = origin[1] + Math.floor(index / 4);
      const z = origin[2] + (index % 2);
      matrix.makeTranslation(x, y, z);
      vein.setMatrixAt(index, matrix);
    }
    vein.instanceMatrix.needsUpdate = true;
    this.quarry.add(vein);
  }

  private createBeacon(): Mesh {
    const geometry = new RingGeometry(2.6, 2.85, 48);
    const material = new MeshStandardMaterial({
      color: 0x57d4a5,
      emissive: 0x1d6b50,
      emissiveIntensity: 1.2,
      roughness: 0.35,
      side: 2,
    });
    const ring = new Mesh(geometry, material);
    ring.rotation.x = -Math.PI / 2;
    ring.position.set(0, -1.42, 0);
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
    const reduced = this.reducedMotion.matches;
    const targetZ = this.mode === "match" ? 14 : 20;
    const targetY = this.mode === "match" ? 7.5 : 12;
    this.camera.position.z += (targetZ - this.camera.position.z) * 0.025;
    this.camera.position.y += (targetY - this.camera.position.y) * 0.025;
    if (!reduced) {
      const time = performance.now() * 0.00018;
      this.camera.position.x = 14 + Math.sin(time) * 1.2 + this.pointerX * 1.4;
      this.quarry.rotation.y =
        Math.sin(time * 0.42) * 0.07 + this.pointerX * 0.03;
      this.camera.position.y += this.pointerY * -0.35;
      this.beacon.rotation.z += 0.002;
    }
    this.camera.lookAt(0, 0.5, 0);
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
    this.liveWorld?.dispose();
    this.liveWorld = null;
    delete this.canvas.dataset.liveWorld;
  }
}
