import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { createInterface, type Interface } from "node:readline";
import { fileURLToPath } from "node:url";

import { sampleProcessGroupRssBytes } from "./process-rss";
import {
  bindAddress,
  type LiveServerOptions,
  type SettlementCrashPoint,
  validateLiveServerOptions,
} from "./server-process-config";

const REPOSITORY_ROOT = fileURLToPath(new URL("../../../../", import.meta.url));
const STARTUP_TIMEOUT_MS = 600_000;
const CONTROL_TIMEOUT_MS = 10_000;
const TERMINATION_GRACE_MS = 5_000;
const ENGINE_TICK_SETTLE_MS = 125;
const MAX_ADVANCE_MS = 15 * 60 * 1_000;
const LOG_TAIL_LINES = 80;

interface ClockAck {
  advance_ms: number;
  event: "e2e_clock_ack";
  monotonic_ms: number;
  status: "ok";
  utc_unix_ms: number;
}

interface PendingAck {
  reject(error: Error): void;
  resolve(ack: ClockAck): void;
  timer: ReturnType<typeof setTimeout>;
}

/**
 * 测试进程持有唯一的可控时钟入口；生产网络表面不会暴露推进时间的能力。
 */
export class ControlledLiveServer {
  private readonly pendingAcks: PendingAck[] = [];
  private readonly logTail: string[] = [];
  private readonly stdoutLines: Interface;
  private readonly stderrLines: Interface;
  private exitStatus: {
    code: number | null;
    signal: NodeJS.Signals | null;
  } | null = null;
  private stopped = false;

  private constructor(
    private readonly child: ChildProcessWithoutNullStreams,
    private readonly options: LiveServerOptions,
  ) {
    this.stdoutLines = createInterface({ input: child.stdout });
    this.stderrLines = createInterface({ input: child.stderr });
    this.stdoutLines.on("line", (line) => this.handleStdout(line));
    this.stderrLines.on("line", (line) => this.remember(line));
    child.once("error", (error) => {
      this.stopped = true;
      this.rejectPending(error);
    });
    child.once("exit", (code, signal) => {
      this.exitStatus = { code, signal };
      this.stopped = true;
      this.rejectPending(
        new Error(
          `Engine 服务提前退出（exit=${String(code)}, signal=${String(signal)}）`,
        ),
      );
    });
  }

  static async start(
    options: LiveServerOptions,
  ): Promise<ControlledLiveServer> {
    validateLiveServerOptions(options);
    const child = spawn(
      "cargo",
      [
        "run",
        "--manifest-path",
        "apps/extraction-server/Cargo.toml",
        "--bin",
        "voxelize-extraction-server",
        "--features",
        "engine,e2e-control",
        "--locked",
      ],
      {
        cwd: REPOSITORY_ROOT,
        detached: process.platform !== "win32",
        env: {
          ...process.env,
          DATABASE_URL: options.databaseUrl,
          EXTRACTION_AUTH_LOGIN_ENABLED: "true",
          EXTRACTION_COOKIE_SECURE: "false",
          EXTRACTION_MATCHMAKING_ENABLED: "true",
          EXTRACTION_PUBLIC_ORIGIN: options.publicOrigin,
          EXTRACTION_SERVER_BIND: bindAddress(options.serverOrigin),
          // 显式清空父进程同名变量，避免普通真实 E2E 误继承故障注入。
          EXTRACTION_E2E_SETTLEMENT_CRASH: options.settlementCrashPoint ?? "",
          EXTRACTION_SIWE_DOMAIN: new URL(options.publicOrigin).host,
          EXTRACTION_SIWE_URI: options.publicOrigin,
          LOG_FORMAT: "json",
          RUST_BACKTRACE: "1",
          RUST_LOG: "warn",
        },
        stdio: ["pipe", "pipe", "pipe"],
      },
    );
    const server = new ControlledLiveServer(child, options);
    try {
      await server.waitUntilReady();
      return server;
    } catch (error) {
      await server.stop();
      throw error;
    }
  }

  advance(milliseconds: number): Promise<ClockAck> {
    if (
      !Number.isSafeInteger(milliseconds) ||
      milliseconds < 0 ||
      milliseconds > MAX_ADVANCE_MS
    ) {
      throw new Error(`时钟推进量必须是 0..${MAX_ADVANCE_MS} 的安全整数`);
    }
    if (this.stopped || !this.child.stdin.writable) {
      throw new Error("Engine 服务已停止，不能推进时钟");
    }
    if (this.pendingAcks.length !== 0) {
      throw new Error("上一条时钟命令尚未确认");
    }
    return new Promise<ClockAck>((resolvePromise, reject) => {
      const timer = setTimeout(() => {
        this.pendingAcks.shift();
        reject(new Error(`等待时钟 ACK 超时\n${this.diagnostics()}`));
      }, CONTROL_TIMEOUT_MS);
      this.pendingAcks.push({ reject, resolve: resolvePromise, timer });
      this.child.stdin.write(`advance_ms ${milliseconds}\n`, (error) => {
        if (error !== null && error !== undefined) {
          const pending = this.pendingAcks.shift();
          if (pending !== undefined) clearTimeout(pending.timer);
          reject(error);
        }
      });
    });
  }

  async elapse(milliseconds: number): Promise<{ monotonicMs: number }> {
    const ack = await this.advance(milliseconds);
    // ACK 先于 Matchmaking ticker 与 World ECS 消费新时间，需等待至少一个调度周期。
    await delay(ENGINE_TICK_SETTLE_MS);
    return { monotonicMs: ack.monotonic_ms };
  }

  async stop(): Promise<void> {
    if (this.stopped) return;
    this.sendSignal("SIGTERM");
    const stopped = await this.waitForStopped(TERMINATION_GRACE_MS);
    if (!stopped) {
      this.sendSignal("SIGKILL");
      await this.waitForStopped(2_000);
    }
    if (!this.stopped) {
      throw new Error(`Engine 服务未能在期限内停止\n${this.diagnostics()}`);
    }
    this.stdoutLines.close();
    this.stderrLines.close();
  }

  async expectSettlementCrash(
    point: SettlementCrashPoint,
    timeoutMs = CONTROL_TIMEOUT_MS,
  ): Promise<void> {
    if (!(await this.waitForStopped(timeoutMs))) {
      throw new Error(`Engine 未在 ${point} 故障点退出\n${this.diagnostics()}`);
    }
    const status = this.exitStatus;
    if (
      status === null ||
      status.code === null ||
      status.code === 0 ||
      status.signal !== null
    ) {
      throw new Error(
        `Engine 退出状态不符合结算故障注入（exit=${String(status?.code)}, signal=${String(status?.signal)}）\n${this.diagnostics()}`,
      );
    }
    const marker = `"event":"e2e_settlement_crash","point":"${point}"`;
    if (!this.diagnostics().includes(marker)) {
      throw new Error(
        `Engine 未记录预期的 ${point} 故障边界\n${this.diagnostics()}`,
      );
    }
  }

  diagnostics(): string {
    return this.logTail.join("\n");
  }

  sampleRssBytes(): Promise<number | null> {
    const pid = this.child.pid;
    if (pid === undefined) throw new Error("Engine 服务没有可采样的进程 ID");
    return sampleProcessGroupRssBytes(pid);
  }

  private async waitUntilReady(): Promise<void> {
    const deadline = Date.now() + STARTUP_TIMEOUT_MS;
    let lastResult = "尚未响应";
    while (Date.now() < deadline) {
      if (this.stopped) {
        throw new Error(`Engine 服务在就绪前退出\n${this.diagnostics()}`);
      }
      try {
        const response = await fetch(
          `${this.options.serverOrigin}/health/ready`,
          {
            headers: { Origin: this.options.publicOrigin },
            signal: AbortSignal.timeout(2_000),
          },
        );
        lastResult = `HTTP ${response.status}`;
        await response.arrayBuffer();
        if (response.ok) return;
      } catch (error) {
        lastResult = error instanceof Error ? error.message : String(error);
      }
      await delay(250);
    }
    throw new Error(
      `Engine readiness 超时：${lastResult}\n${this.diagnostics()}`,
    );
  }

  private handleStdout(line: string): void {
    this.remember(line);
    let value: unknown;
    try {
      value = JSON.parse(line) as unknown;
    } catch {
      return;
    }
    if (!isClockAck(value)) return;
    const pending = this.pendingAcks.shift();
    if (pending === undefined) {
      this.remember("收到没有对应命令的时钟 ACK");
      return;
    }
    clearTimeout(pending.timer);
    pending.resolve(value);
  }

  private remember(line: string): void {
    this.logTail.push(line);
    if (this.logTail.length > LOG_TAIL_LINES) this.logTail.shift();
  }

  private rejectPending(error: Error): void {
    for (const pending of this.pendingAcks.splice(0)) {
      clearTimeout(pending.timer);
      pending.reject(error);
    }
  }

  private sendSignal(signal: NodeJS.Signals): void {
    const pid = this.child.pid;
    if (pid === undefined || this.stopped) return;
    try {
      if (process.platform === "win32") this.child.kill(signal);
      else process.kill(-pid, signal);
    } catch (error) {
      if ((error as NodeJS.ErrnoException).code !== "ESRCH") throw error;
    }
  }

  private async waitForStopped(timeoutMs: number): Promise<boolean> {
    const deadline = Date.now() + timeoutMs;
    while (!this.stopped && Date.now() < deadline) await delay(50);
    return this.stopped;
  }
}

function isClockAck(value: unknown): value is ClockAck {
  if (typeof value !== "object" || value === null) return false;
  const source = value as Record<string, unknown>;
  return (
    source.event === "e2e_clock_ack" &&
    source.status === "ok" &&
    typeof source.advance_ms === "number" &&
    typeof source.monotonic_ms === "number" &&
    typeof source.utc_unix_ms === "number"
  );
}

function delay(milliseconds: number): Promise<void> {
  return new Promise((resolvePromise) =>
    setTimeout(resolvePromise, milliseconds),
  );
}
