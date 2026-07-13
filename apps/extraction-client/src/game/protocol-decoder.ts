import { decodeMessage } from "@voxelize/core/decode-message";
import type { MessageProtocol } from "@voxelize/protocol";

import ProtocolWorker from "./protocol-worker?worker&inline";

type DecodeResponse =
  | {
      error?: never;
      generation: number;
      message: MessageProtocol;
      sequence: number;
    }
  | {
      error: true;
      generation: number;
      message?: never;
      sequence: number;
    };

export class ProtocolDecoder {
  private worker: Worker | null;
  private readonly pending = new Map<number, DecodeResponse>();
  private generation = 0;
  private sequence = 0;
  private nextSequence = 0;
  private disposed = false;

  constructor(
    private readonly onMessage: (message: MessageProtocol) => void,
    private readonly onError: () => void,
  ) {
    this.worker =
      typeof Worker === "undefined"
        ? null
        : new ProtocolWorker({ name: "pvp-protocol" });
    this.worker?.addEventListener("message", this.handleWorkerMessage);
    this.worker?.addEventListener("error", this.handleWorkerError);
  }

  decode(buffer: ArrayBuffer): void {
    if (this.disposed) return;
    const sequence = this.sequence;
    this.sequence += 1;
    const generation = this.generation;
    if (this.worker !== null) {
      this.worker.postMessage({ buffer, generation, sequence }, [buffer]);
      return;
    }
    queueMicrotask(() =>
      this.decodeWithoutWorker(buffer, generation, sequence),
    );
  }

  reset(): void {
    this.generation += 1;
    this.sequence = 0;
    this.nextSequence = 0;
    this.pending.clear();
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.pending.clear();
    this.worker?.removeEventListener("message", this.handleWorkerMessage);
    this.worker?.removeEventListener("error", this.handleWorkerError);
    this.worker?.terminate();
  }

  private handleWorkerMessage = (event: MessageEvent<DecodeResponse>): void => {
    this.accept(event.data);
  };

  private accept(response: DecodeResponse): void {
    if (response.generation !== this.generation || this.disposed) return;
    this.pending.set(response.sequence, response);
    this.flush();
  }

  private handleWorkerError = (): void => {
    if (this.disposed || this.worker === null) return;
    this.worker.removeEventListener("message", this.handleWorkerMessage);
    this.worker.removeEventListener("error", this.handleWorkerError);
    this.worker.terminate();
    this.worker = null;
    this.reset();
    this.onError();
  };

  private decodeWithoutWorker(
    buffer: ArrayBuffer,
    generation: number,
    sequence: number,
  ): void {
    try {
      const message = decodeMessage(
        new Uint8Array(buffer),
        [],
      ) as MessageProtocol;
      this.accept({ generation, sequence, message });
    } catch {
      this.accept({ generation, sequence, error: true });
    }
  }

  private flush(): void {
    for (;;) {
      const response = this.pending.get(this.nextSequence);
      if (response === undefined) return;
      this.pending.delete(this.nextSequence);
      this.nextSequence += 1;
      if (response.error === true) this.onError();
      else this.onMessage(response.message);
    }
  }
}
