import { randomUUID } from "node:crypto";

import WebSocket, { type RawData } from "ws";

import { decodeAttackResultData } from "../../../../contracts/extraction/v1/combat";
import { decodeProtocolEnvelope } from "../../../../contracts/extraction/v1/envelope";
import { decodeGameplayStateData } from "../../../../contracts/extraction/v1/gameplay-state";
import type {
  AttackResultData,
  ExtractionManifest,
  GameplayStateData,
  MiningPayload,
  ResultEnvelope,
} from "../../../../contracts/extraction/v1/typescript";

import {
  checkedNextU32,
  GameplaySocketClosedError,
  LiveGameplayError,
} from "./gameplay-errors";
import { GameplayFrameStore } from "./gameplay-frame-store";
import { acceptedGameplaySequence, type LiveVector3 } from "./gameplay-state";
import {
  decodeServerFrame,
  encodeMethod,
  encodeMovement,
  rawDataToBytes,
  type LiveMovementInput,
  type LiveServerFrame,
} from "./wire";

const RESULT_METHOD = "pvp:v1:result";

interface PendingResult {
  reject(error: Error): void;
  resolve(value: ResultEnvelope): void;
  timer: ReturnType<typeof setTimeout>;
}

export class GameplayProtocolDriver {
  readonly playerId: string;
  private readonly frames: GameplayFrameStore;
  private readonly pending = new Map<string, PendingResult>();
  private fatalError: Error | null = null;
  private gameplaySequence = 0;
  private querySequence = 0;
  private disposed = false;

  constructor(
    private readonly socket: WebSocket,
    init: LiveServerFrame,
    readonly matchId: string,
    readonly manifest: ExtractionManifest,
    private readonly timeoutMs: number,
  ) {
    this.frames = new GameplayFrameStore(init);
    this.playerId = this.frames.playerId;
    socket.on("message", this.onMessage);
    socket.once("close", this.onClose);
    socket.once("error", this.onError);
  }

  position(): LiveVector3 {
    const position = this.frames.position();
    if (position === null)
      throw new Error("own authoritative position is unavailable");
    return position;
  }

  peerPosition(playerId: string): LiveVector3 | null {
    return this.frames.position(playerId);
  }

  visibleLoot() {
    return this.frames.visibleLoot();
  }

  observedLoot(): string[] {
    return this.frames.observedLoot();
  }

  lootCreationCount(id: string): number {
    return this.frames.lootCreationCount(id);
  }

  removedLoot(): string[] {
    return this.frames.removedLoot();
  }

  latestMethodState(name: string): unknown {
    return this.frames.latestMethodState(name);
  }

  async sendMovement(input: LiveMovementInput): Promise<void> {
    await this.send(encodeMovement(input));
  }

  async sendMovementAndWaitForDirection(
    input: LiveMovementInput,
    predicate: (direction: LiveVector3) => boolean,
    message: string,
  ): Promise<LiveVector3> {
    const previousRevision = this.frames.peerRevision();
    await this.sendMovement(input);
    return this.waitUntil(() => {
      const direction = this.frames.direction();
      if (
        this.frames.peerRevision() <= previousRevision ||
        direction === null ||
        !predicate(direction)
      ) {
        return null;
      }
      return this.frames.position();
    }, message);
  }

  async getState(): Promise<GameplayStateData> {
    this.querySequence = checkedNextU32(this.querySequence, "query sequence");
    const data = await this.request("pvp:v1:get-state", {}, this.querySequence);
    const state = decodeGameplayStateData(data, this.manifest);
    if (state.matchId !== this.matchId) {
      throw new Error("get-state returned another match");
    }
    this.gameplaySequence = Math.max(
      this.gameplaySequence,
      acceptedGameplaySequence(state),
    );
    return state;
  }

  async attack(): Promise<AttackResultData> {
    const sequence = this.nextGameplaySequence();
    const data = await this.request(
      "pvp:v1:attack",
      { weaponSlot: "melee" },
      sequence,
    );
    const result = decodeAttackResultData(data);
    if (result.acceptedSequence !== sequence) {
      throw new Error("attack result returned another sequence");
    }
    return result;
  }

  async mining(payload: MiningPayload): Promise<void> {
    await this.request("pvp:v1:mining", payload, this.nextGameplaySequence());
  }

  async dropSlot(
    slot: number,
    expectedInventoryRevision: number,
  ): Promise<void> {
    await this.request(
      "pvp:v1:drop-slot",
      { slot, expectedInventoryRevision },
      this.nextGameplaySequence(),
    );
  }

  async waitForPosition(
    predicate: (position: LiveVector3) => boolean,
    message: string,
  ): Promise<LiveVector3> {
    return this.waitUntil(() => {
      const position = this.frames.position();
      return position !== null && predicate(position) ? position : null;
    }, message);
  }

  async waitForMethodState<T>(
    name: string,
    decode: (value: unknown) => T,
    predicate: (value: T) => boolean,
    message: string,
  ): Promise<T> {
    return this.waitUntil(() => {
      if (!this.frames.hasMethodState(name)) return null;
      const value = decode(this.frames.latestMethodState(name));
      return predicate(value) ? value : null;
    }, message);
  }

  /** 模拟网络直接中断，保留服务端角色以进入真实 detach/rebind 路径。 */
  async disconnectTransport(): Promise<void> {
    if (this.socket.readyState === WebSocket.CLOSED) return;
    const closed = new Promise<void>((resolve) =>
      this.socket.once("close", () => resolve()),
    );
    this.socket.terminate();
    await closed;
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.socket.off("message", this.onMessage);
    this.socket.off("close", this.onClose);
    this.socket.off("error", this.onError);
    this.fail(new Error("gameplay driver disposed"));
  }

  private readonly onMessage = (data: RawData, isBinary: boolean): void => {
    if (!isBinary)
      return this.fail(new Error("server sent a text gameplay frame"));
    try {
      this.applyFrame(decodeServerFrame(rawDataToBytes(data)));
    } catch (error) {
      this.fail(
        error instanceof Error ? error : new Error("gameplay decode failed"),
      );
    }
  };

  private readonly onClose = (): void => {
    this.fail(new GameplaySocketClosedError());
  };

  private readonly onError = (error: Error): void => this.fail(error);

  private applyFrame(frame: LiveServerFrame): void {
    const method = this.frames.apply(frame);
    if (method === null || method.name !== RESULT_METHOD) return;
    const envelope = decodeProtocolEnvelope(method.payload, this.manifest);
    if (envelope.type !== "result")
      throw new Error("result Method carried an intent");
    const pending = this.pending.get(envelope.requestId);
    if (pending === undefined) return;
    clearTimeout(pending.timer);
    this.pending.delete(envelope.requestId);
    pending.resolve(envelope);
  }

  private async request(
    name: string,
    payload: object,
    sequence: number,
  ): Promise<unknown> {
    const requestId = randomUUID();
    const envelope = {
      protocolVersion: this.manifest.protocolVersion,
      type: "intent",
      requestId,
      sequence,
      payload,
    } as const;
    const result = new Promise<ResultEnvelope>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(requestId);
        reject(new Error(`timed out waiting for ${name} result`));
      }, this.timeoutMs);
      this.pending.set(requestId, { reject, resolve, timer });
    });
    try {
      await this.send(encodeMethod(name, envelope));
    } catch (error) {
      const pending = this.pending.get(requestId);
      if (pending !== undefined) clearTimeout(pending.timer);
      this.pending.delete(requestId);
      throw error;
    }
    const response = await result;
    if (response.outcome.status === "error") {
      throw new LiveGameplayError(
        response.outcome.error.code,
        response.outcome.error.retryable,
      );
    }
    return response.outcome.data;
  }

  private nextGameplaySequence(): number {
    this.gameplaySequence = checkedNextU32(
      this.gameplaySequence,
      "gameplay sequence",
    );
    return this.gameplaySequence;
  }

  private send(bytes: Uint8Array): Promise<void> {
    if (this.fatalError !== null) return Promise.reject(this.fatalError);
    if (this.socket.readyState !== WebSocket.OPEN) {
      return Promise.reject(new Error("gameplay WebSocket is not open"));
    }
    return new Promise((resolve, reject) => {
      this.socket.send(bytes, { binary: true }, (error) => {
        if (error == null) resolve();
        else reject(error);
      });
    });
  }

  private async waitUntil<T>(
    read: () => T | null,
    message: string,
  ): Promise<T> {
    const deadline = Date.now() + this.timeoutMs;
    for (;;) {
      const value = read();
      if (value !== null) return value;
      // 终态 Direct 可能与 policy close 同一批到达；缓存证据优先于关闭错误。
      if (this.fatalError !== null) throw this.fatalError;
      if (Date.now() >= deadline) throw new Error(message);
      await new Promise((resolve) => setTimeout(resolve, 10));
    }
  }

  private fail(error: Error): void {
    this.fatalError ??= error;
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timer);
      pending.reject(this.fatalError);
    }
    this.pending.clear();
  }
}
