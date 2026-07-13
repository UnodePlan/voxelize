import { Inputs, RigidControls, VoxelInteract, World } from "@voxelize/core";
import type { MessageProtocol } from "@voxelize/protocol";
import { Color, PerspectiveCamera, Vector3, type WebGLRenderer } from "three";

import type { ExtractionManifest } from "../../../../contracts/extraction/v1/typescript";

import { disposeObjectTree } from "./object-disposal";
import { createWorldActors } from "./world-actors";
import { WorldInputController, type WorldInputActions } from "./world-input";
import { runWorldSessionCleanup } from "./world-session-disposal";
import { applyExtractionTextures } from "./world-textures";

export interface WorldSessionActions extends WorldInputActions {
  onError(message: string): void;
  onWorldReady(): void;
  sendWorldPacket(message: MessageProtocol): void;
}

interface ClientInfo {
  id: string;
  metadata: Record<string, never>;
  username: string;
}

const AUTHORITY_SNAP_DISTANCE = 0.5;
const PLAYER_BODY_WIDTH = 0.8;
const PLAYER_BODY_HEIGHT = 1.8;
const PLAYER_EYE_HEIGHT_RATIO = 0.9;
const PLAYER_SPEED = 6;

export class VoxelWorldSession {
  readonly camera = new PerspectiveCamera(76, 1, 0.1, 700);
  private readonly world = new World({ textureUnitDimension: 8 });
  private readonly controls: RigidControls;
  private readonly inputs = new Inputs<"disabled" | "in-game">();
  private readonly interact: VoxelInteract;
  private readonly actors;
  private readonly input;
  private readonly disconnectControls;
  private readonly direction = new Vector3();
  private readonly clientInfo: ClientInfo = {
    id: "",
    metadata: {},
    username: "Extractor",
  };
  private messageChain = Promise.resolve();
  private disposed = false;
  private initialized = false;

  constructor(
    private readonly canvas: HTMLCanvasElement,
    private readonly manifest: ExtractionManifest,
    private readonly actions: WorldSessionActions,
  ) {
    this.controls = new RigidControls(this.camera, canvas, this.world, {
      bodyDepth: PLAYER_BODY_WIDTH,
      bodyHeight: PLAYER_BODY_HEIGHT,
      bodyWidth: PLAYER_BODY_WIDTH,
      crouchBodyHeight: PLAYER_BODY_HEIGHT,
      crouchFactor: 1,
      eyeHeight: PLAYER_EYE_HEIGHT_RATIO,
      initialPosition: [0, 50, 0],
      maxSpeed: PLAYER_SPEED,
      sprintFactor: 1,
      stepHeight: 0,
    });
    this.disconnectControls = this.controls.connect(this.inputs, "in-game");
    this.interact = new VoxelInteract(this.controls.object, this.world, {
      highlightColor: new Color("#effff7"),
      highlightOpacity: 0.64,
      highlightType: "outline",
      ignoreFluids: true,
      inverseDirection: true,
      potentialVisuals: false,
      reachDistance: 4.5,
    });
    this.world.add(this.interact);
    this.actors = createWorldActors(this.world, this.controls, manifest);
    this.input = new WorldInputController(
      canvas,
      this.controls,
      this.inputs,
      this.interact,
      this.actors.peers,
      actions,
    );
  }

  get ready(): boolean {
    return this.initialized && !this.disposed;
  }

  enqueue(message: MessageProtocol): void {
    this.messageChain = this.messageChain
      .then(() => this.applyMessage(message))
      .catch(() => {
        if (!this.disposed) this.actions.onError("体素世界同步失败");
      });
  }

  update(): void {
    if (!this.ready) return;
    this.interact.update();
    this.controls.update();
    this.world.update(
      this.controls.object.position,
      this.camera.getWorldDirection(this.direction),
    );
    this.actors.peers.update();
    this.actors.entities.update(
      this.controls.object.position,
      this.world.renderRadius * this.world.options.chunkSize,
    );
    this.input.update();
    this.flushWorldPackets();
  }

  render(renderer: WebGLRenderer): void {
    if (this.ready) renderer.render(this.world, this.camera);
  }

  resize(width: number, height: number): void {
    this.camera.aspect = width / height;
    this.camera.updateProjectionMatrix();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.initialized = false;
    const error = runWorldSessionCleanup([
      () => this.input.dispose(),
      () => this.disconnectControls(),
      () => this.inputs.reset(),
      () => disposeObjectTree(this.interact, true),
      () => disposeObjectTree(this.actors.entities, false),
      () => disposeObjectTree(this.actors.peers, true),
      () => this.world.dispose(),
    ]);
    if (error !== null) this.actions.onError("体素世界资源释放失败");
  }

  private async applyMessage(message: MessageProtocol): Promise<void> {
    if (this.disposed) return;
    if (message.type === "INIT") {
      await this.initialize(message);
      return;
    }
    if (!this.initialized) return;
    this.routeMessage(message);
    this.reconcileAuthority(message);
  }

  private async initialize(message: MessageProtocol): Promise<void> {
    const init = record(message.json);
    const id = init?.id;
    if (typeof id !== "string" || id.length === 0) {
      throw new Error("INIT 缺少玩家 ID");
    }
    this.clientInfo.id = id;
    this.world.onMessage(message);
    this.controls.onMessage(message);
    this.actors.peers.onMessage(message, this.clientInfo);
    await this.world.initialize();
    if (this.disposed) return;
    await applyExtractionTextures(this.world, this.manifest);
    if (this.disposed) return;
    this.actors.entities.onMessage(message);
    this.restorePose();
    this.world.renderRadius = 6;
    this.initialized = true;
    this.canvas.dataset.liveWorld = "ready";
    this.actions.onWorldReady();
    this.flushWorldPackets();
  }

  private routeMessage(message: MessageProtocol): void {
    this.world.onMessage(message);
    this.controls.onMessage(message);
    this.actors.peers.onMessage(message, this.clientInfo);
    this.actors.entities.onMessage(message);
  }

  private restorePose(): void {
    const position = vector(this.world.extraInitData.savedPosition);
    const direction = vector(this.world.extraInitData.savedDirection);
    if (position !== null) this.controls.teleportToExact(...position);
    if (direction !== null) this.controls.setDirection(...direction);
  }

  private reconcileAuthority(message: MessageProtocol): void {
    if (message.type !== "PEER" || !Array.isArray(message.peers)) return;
    const own = message.peers.find((peer) => peer.id === this.clientInfo.id);
    const metadata = record(own?.metadata);
    const position = vector(metadata?.position);
    if (position === null) return;
    const current = this.controls.object.position;
    if (
      current.distanceTo(new Vector3(...position)) > AUTHORITY_SNAP_DISTANCE
    ) {
      this.controls.teleportToExact(...position);
    }
  }

  private flushWorldPackets(): void {
    const packets = this.world.packets.splice(0, this.world.packets.length);
    for (const packet of packets) {
      if (packet.type === "LOAD" || packet.type === "UNLOAD") {
        this.actions.sendWorldPacket(packet);
      }
    }
  }
}

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function vector(value: unknown): [number, number, number] | null {
  return Array.isArray(value) &&
    value.length === 3 &&
    value.every((part) => typeof part === "number" && Number.isFinite(part))
    ? [value[0] as number, value[1] as number, value[2] as number]
    : null;
}
