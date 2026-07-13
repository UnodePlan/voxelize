import type { Page } from "@playwright/test";

import { installWebGlProbe, readWebGlProbe } from "./visual-gate-webgl-probe";
import {
  installVisualWorkerProbe,
  readVisualWorkerProbe,
  type WorkerObservation,
  type WorkerProbeError,
} from "./visual-gate-worker-probe";
import { decodeServerFrame } from "./wire";

const CRITICAL_RESOURCE_TYPES = new Set([
  "document",
  "fetch",
  "font",
  "image",
  "script",
  "stylesheet",
  "xhr",
]);
const DEFAULT_SNAPSHOT_TIMEOUT_MS = 5_000;

export interface RuntimeDiagnosticsSnapshot {
  activeGameSocketCount: number;
  closedGameSocketCount: number;
  errors: string[];
  evaluableWorkerCount: number;
  gameSocketCount: number;
  liveWorkerCount: number;
  meshResponseCount: number;
  nonEmptyMeshResponseCount: number;
  observedWorkers: WorkerObservation[];
  observedWorkerUrls: string[];
  webglCanvasCount: number;
  webglContextCount: number;
  webglContextLostCount: number;
  webglContextRestoredCount: number;
  webglContextTypes: string[];
  workerErrors: WorkerProbeError[];
  workerEvaluationErrors: string[];
  workerUrls: string[];
  workers: WorkerRuntimeCheck[];
}

export interface WorkerRuntimeCheck {
  evaluable: boolean;
  name: string;
  url: string;
}

/** 收集真实页面运行时信号，不向产品客户端注入测试桥接状态。 */
export class LiveRuntimeDiagnostics {
  private readonly errors = new Set<string>();
  private readonly observedWorkerUrls = new Set<string>();
  private gameSocketCount = 0;
  private activeGameSocketCount = 0;
  private closedGameSocketCount = 0;
  private expectedGameSocketCloses = 0;
  private gamePlayerId: string | null = null;
  private closing = false;

  private constructor(
    private readonly page: Page,
    private readonly clientOrigin: string,
  ) {
    page.on("pageerror", (error) => {
      this.record(`pageerror: ${error.message}`);
    });
    page.on("console", (message) => {
      if (
        message.type() === "error" &&
        !isBrowserResourceConsoleError(message.text())
      ) {
        this.record(`console.error: ${message.text()}`);
      }
    });
    page.on("requestfailed", (request) => {
      const resourceType = request.resourceType();
      const failureText = request.failure()?.errorText ?? "unknown";
      if (
        this.isCriticalHttp(request.url(), resourceType) &&
        !isCancelledApiRequest(resourceType, failureText)
      ) {
        this.record(
          `requestfailed: ${request.method()} ${request.url()} (${failureText})`,
        );
      }
    });
    page.on("response", (response) => {
      const request = response.request();
      if (
        response.status() >= 400 &&
        this.isCriticalHttp(response.url(), request.resourceType())
      ) {
        this.record(
          `HTTP ${response.status()}: ${request.method()} ${response.url()}`,
        );
      }
    });
    page.on("worker", (worker) => {
      this.observedWorkerUrls.add(describeWorkerUrl(worker.url()));
    });
    page.on("websocket", (socket) => {
      if (!isGameSocket(socket.url())) return;
      this.gameSocketCount += 1;
      this.activeGameSocketCount += 1;
      socket.on("framereceived", ({ payload }) => {
        if (typeof payload === "string") return;
        try {
          const bytes = new Uint8Array(
            payload.buffer,
            payload.byteOffset,
            payload.byteLength,
          );
          const frame = decodeServerFrame(bytes);
          if (frame.type !== "INIT") return;
          const playerId = readInitPlayerId(frame.json);
          if (this.gamePlayerId !== null && this.gamePlayerId !== playerId) {
            this.record("game websocket replaced its public player ID");
          } else {
            this.gamePlayerId = playerId;
          }
        } catch (error) {
          this.record(
            `game websocket frame decode failed: ${error instanceof Error ? error.message : String(error)}`,
          );
        }
      });
      socket.on("socketerror", (error) => {
        this.record(`game websocket error: ${String(error)}`);
      });
      socket.on("close", () => {
        this.activeGameSocketCount = Math.max(
          0,
          this.activeGameSocketCount - 1,
        );
        this.closedGameSocketCount += 1;
        if (this.closing) return;
        if (this.expectedGameSocketCloses > 0) {
          this.expectedGameSocketCloses -= 1;
          return;
        }
        this.record(
          `game websocket closed before expected exit: ${socket.url()}`,
        );
      });
    });
  }

  static async create(
    page: Page,
    clientOrigin: string,
  ): Promise<LiveRuntimeDiagnostics> {
    await Promise.all([
      installVisualWorkerProbe(page),
      installWebGlProbe(page),
    ]);
    return new LiveRuntimeDiagnostics(page, clientOrigin);
  }

  assertNoFailures(actorId: string): void {
    if (this.errors.size > 0) {
      throw new Error(
        `${actorId} product client failed: ${[...this.errors].join(" | ")}`,
      );
    }
  }

  markClosing(): void {
    this.closing = true;
  }

  beginExpectedMatch(): void {
    this.gamePlayerId = null;
  }

  expectGameSocketClose(): void {
    this.expectedGameSocketCloses += 1;
  }

  requireGamePlayerId(actorId: string): string {
    if (this.gamePlayerId === null) {
      throw new Error(`${actorId} INIT public player ID was not observed`);
    }
    return this.gamePlayerId;
  }

  async snapshot(
    timeoutMs = DEFAULT_SNAPSHOT_TIMEOUT_MS,
  ): Promise<RuntimeDiagnosticsSnapshot> {
    if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0) {
      throw new Error("runtime diagnostics timeout must be a positive integer");
    }
    return withTimeout(
      this.collectSnapshot(),
      timeoutMs,
      "runtime diagnostics snapshot",
    );
  }

  private async collectSnapshot(): Promise<RuntimeDiagnosticsSnapshot> {
    const workers = this.page.workers();
    const [checks, probe, webgl] = await Promise.all([
      Promise.all(
        workers.map(async (worker) => {
          const url = describeWorkerUrl(worker.url());
          try {
            const result = await worker.evaluate(() => ({
              evaluable:
                typeof self === "object" && typeof postMessage === "function",
              name: typeof self.name === "string" ? self.name : "",
            }));
            return { error: null, url, ...result };
          } catch (error) {
            return {
              error: error instanceof Error ? error.message : String(error),
              evaluable: false,
              name: "",
              url,
            };
          }
        }),
      ),
      readVisualWorkerProbe(this.page),
      readWebGlProbe(this.page),
    ]);
    const runtimeWorkers = checks.map(({ evaluable, name, url }) => ({
      evaluable,
      name,
      url,
    }));
    return {
      activeGameSocketCount: this.activeGameSocketCount,
      closedGameSocketCount: this.closedGameSocketCount,
      errors: [...this.errors],
      evaluableWorkerCount: checks.filter(({ evaluable }) => evaluable).length,
      gameSocketCount: this.gameSocketCount,
      liveWorkerCount: workers.length,
      meshResponseCount: probe.meshResponseCount,
      nonEmptyMeshResponseCount: probe.nonEmptyMeshResponseCount,
      observedWorkers: probe.observed,
      observedWorkerUrls: [...this.observedWorkerUrls],
      webglCanvasCount: webgl.canvasCount,
      webglContextCount: webgl.contextCount,
      webglContextLostCount: webgl.contextLostCount,
      webglContextRestoredCount: webgl.contextRestoredCount,
      webglContextTypes: webgl.contextTypes,
      workerErrors: probe.errors,
      workerEvaluationErrors: checks.flatMap(({ error, url }) =>
        error === null ? [] : [`${url || "inline worker"}: ${error}`],
      ),
      workerUrls: runtimeWorkers.map(({ url }) => url),
      workers: runtimeWorkers,
    };
  }

  private isCriticalHttp(url: string, resourceType: string): boolean {
    try {
      const parsed = new URL(url);
      return (
        parsed.origin === this.clientOrigin &&
        (CRITICAL_RESOURCE_TYPES.has(resourceType) ||
          parsed.pathname.endsWith(".wasm"))
      );
    } catch {
      return false;
    }
  }

  private record(message: string): void {
    if (!this.closing) this.errors.add(message);
  }
}

function withTimeout<T>(
  operation: Promise<T>,
  timeoutMs: number,
  label: string,
): Promise<T> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(
      () => reject(new Error(`${label} exceeded ${timeoutMs}ms`)),
      timeoutMs,
    );
    operation.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (error: unknown) => {
        clearTimeout(timer);
        reject(error);
      },
    );
  });
}

function isGameSocket(value: string): boolean {
  try {
    return new URL(value).pathname.startsWith("/ws/");
  } catch {
    return false;
  }
}

function readInitPlayerId(value: unknown): string {
  if (typeof value !== "object" || value === null) {
    throw new Error("INIT json is missing");
  }
  const id = Reflect.get(value, "id");
  if (typeof id !== "string" || id.length === 0 || id.length > 160) {
    throw new Error("INIT public player ID is invalid");
  }
  return id;
}

function describeWorkerUrl(value: string): string {
  if (value.length <= 160) return value;
  return `${value.slice(0, 120)}...(${value.length} chars)`;
}

function isBrowserResourceConsoleError(value: string): boolean {
  return value.startsWith("Failed to load resource:");
}

function isCancelledApiRequest(
  resourceType: string,
  failureText: string,
): boolean {
  return (
    (resourceType === "fetch" || resourceType === "xhr") &&
    failureText.includes("ERR_ABORTED")
  );
}
