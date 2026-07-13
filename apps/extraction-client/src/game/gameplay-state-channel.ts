import {
  decodeGameplayStateData,
  decodeProtocolEnvelope,
  type ExtractionManifest,
  type GameplayStateData,
} from "../../../../contracts/extraction/v1/typescript";

import { createIntent } from "./network-protocol";
import type { PendingRequest } from "./network-types";

const REQUEST_TIMEOUT_MS = 3_000;
const MAX_U32 = 4_294_967_295;

export class GameplayStateChannel {
  private pending = new Map<string, PendingRequest>();
  private requestSequence = 0;
  private syncTimer: ReturnType<typeof setTimeout> | null = null;

  constructor(
    private readonly manifest: ExtractionManifest,
    private readonly sendMethod: (name: string, payload: unknown) => void,
    private readonly onState: (state: GameplayStateData) => void,
  ) {}

  request(): Promise<GameplayStateData> {
    const request = createIntent(
      this.manifest.protocolVersion,
      this.nextRequestSequence(),
      {},
    );
    this.sendMethod("pvp:v1:get-state", request);
    return new Promise<GameplayStateData>((resolve, reject) => {
      const timeout = setTimeout(() => {
        this.pending.delete(request.requestId);
        reject(new Error("Gameplay state request timed out"));
      }, REQUEST_TIMEOUT_MS);
      this.pending.set(request.requestId, { reject, resolve, timeout });
    });
  }

  handleResult(value: unknown): void {
    const envelope = decodeProtocolEnvelope(value, this.manifest);
    if (envelope.type !== "result") return;
    const pending = this.pending.get(envelope.requestId);
    if (pending === undefined) return;
    clearTimeout(pending.timeout);
    this.pending.delete(envelope.requestId);
    if (envelope.outcome.status === "error") {
      pending.reject(new Error(envelope.outcome.error.code));
      return;
    }
    let state: GameplayStateData;
    try {
      state = decodeGameplayStateData(envelope.outcome.data, this.manifest);
    } catch (error) {
      pending.reject(
        error instanceof Error ? error : new Error("Gameplay state is invalid"),
      );
      throw error;
    }
    pending.resolve(state);
    this.onState(state);
  }

  scheduleSync(): void {
    if (this.syncTimer !== null) return;
    this.syncTimer = setTimeout(() => {
      this.syncTimer = null;
      try {
        void this.request().catch(() => undefined);
      } catch {
        // Socket 可能在定时器与请求之间关闭；关闭路径已经拒绝全部 pending。
      }
    }, 40);
  }

  rejectAll(message: string): void {
    if (this.syncTimer !== null) clearTimeout(this.syncTimer);
    this.syncTimer = null;
    this.pending.forEach((request) => {
      clearTimeout(request.timeout);
      request.reject(new Error(message));
    });
    this.pending.clear();
  }

  private nextRequestSequence(): number {
    this.requestSequence =
      this.requestSequence >= MAX_U32 ? 0 : this.requestSequence + 1;
    return this.requestSequence;
  }
}
