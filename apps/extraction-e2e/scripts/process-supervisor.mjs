import { spawn as spawnProcess } from "node:child_process";

import { waitForChildExit } from "./child-process-state.mjs";
import { trackOwnedPosixProcessTree } from "./posix-process-tracker.mjs";
import { terminateOwnedPosixProcessTree } from "./posix-process-tree.mjs";
import {
  stopTrackedWindowsProcessTree,
  trackOwnedWindowsProcessTree,
} from "./windows-process-tracker.mjs";
import { terminateWindowsProcessTree } from "./windows-process-tree.mjs";

const DEFAULT_TERMINATION_GRACE_MS = 5_000;
const DEFAULT_KILL_GRACE_MS = 2_000;

export class ProcessSupervisor {
  #active = false;
  #interruptReject = null;
  #interruptedBy = null;
  #ownedGroups = new Map();
  #shutdownPromise = null;
  #signalHandlers = new Map();
  #timer = null;

  constructor({
    cwd,
    environment,
    logPrefix,
    timeoutMs,
    killGraceMs = DEFAULT_KILL_GRACE_MS,
    platform = process.platform,
    signalSource = process,
    spawn = spawnProcess,
    trackProcessTree = trackOwnedPosixProcessTree,
    terminateGroup = terminateOwnedPosixProcessTree,
    terminateWindowsTree = terminateWindowsProcessTree,
    terminationGraceMs = DEFAULT_TERMINATION_GRACE_MS,
    trackWindowsProcessTree = trackOwnedWindowsProcessTree,
  }) {
    requirePositiveInteger(timeoutMs, "timeoutMs");
    requirePositiveInteger(terminationGraceMs, "terminationGraceMs");
    requirePositiveInteger(killGraceMs, "killGraceMs");
    this.cwd = cwd;
    this.environment = environment;
    this.logPrefix = logPrefix;
    this.timeoutMs = timeoutMs;
    this.terminationGraceMs = terminationGraceMs;
    this.killGraceMs = killGraceMs;
    this.platform = platform;
    this.signalSource = signalSource;
    this.spawn = spawn;
    this.trackProcessTree = trackProcessTree;
    this.terminateGroup = terminateGroup;
    this.terminateWindowsTree = terminateWindowsTree;
    this.trackWindowsProcessTree = trackWindowsProcessTree;
  }

  get interruptedBy() {
    return this.#interruptedBy;
  }

  async supervise(operation) {
    if (this.#active) throw new Error("进程监督器不能重复运行");
    this.#active = true;
    this.#installInterrupts();
    const interrupted = new Promise((_, reject) => {
      this.#interruptReject = reject;
    });
    const running = Promise.resolve().then(() => operation(this));
    let value;
    let operationError = null;
    try {
      value = await Promise.race([running, interrupted]);
    } catch (error) {
      operationError = error;
    }

    let cleanupError = null;
    try {
      await this.#shutdownWithRetry();
    } catch (error) {
      cleanupError = error;
    } finally {
      this.#removeInterrupts();
    }
    if (operationError === null && this.#interruptedBy !== null) {
      operationError = new Error(`E2E 运行被 ${this.#interruptedBy} 中止`);
    }
    if (operationError !== null && cleanupError !== null) {
      throw combinedError("E2E 命令失败且进程清理未完成", [
        operationError,
        cleanupError,
      ]);
    }
    if (operationError !== null) throw operationError;
    if (cleanupError !== null) throw cleanupError;
    return value;
  }

  startCommand(label, command, args, options = {}) {
    return this.#startOwnedCommand(label, command, args, options).child;
  }

  async runCommand(label, command, args, options = {}) {
    const { exit, owner } = this.#startOwnedCommand(
      label,
      command,
      args,
      options,
    );
    let result;
    let waitError = null;
    try {
      result = await exit;
    } catch (error) {
      waitError = error;
    }

    let cleanupError = null;
    try {
      await this.#stopOwner(owner);
    } catch (error) {
      cleanupError = error;
    }
    if (waitError !== null && cleanupError !== null) {
      throw combinedError(`${label}失败且进程清理未完成`, [
        waitError,
        cleanupError,
      ]);
    }
    if (waitError !== null) throw waitError;
    if (cleanupError !== null) throw cleanupError;
    if (result.code !== 0) {
      throw new Error(
        `${label}失败（exit=${String(result.code)}, signal=${String(result.signal)}）`,
      );
    }
  }

  #startOwnedCommand(label, command, args, options) {
    if (this.#interruptedBy !== null) {
      throw new Error(`收到 ${this.#interruptedBy} 后禁止启动新的子进程`);
    }
    console.log(`[${this.logPrefix}] 启动${label}`);
    const child = this.spawn(command, args, {
      cwd: this.cwd,
      detached: this.platform !== "win32",
      env: options.environment ?? this.environment,
      stdio: "inherit",
    });
    const owner = this.#rememberOwnedProcess(child);
    const exit = waitForChildExit(child, {
      label,
      timeoutMs: options.timeoutMs ?? null,
    });
    void exit.catch(() => {});
    return { child, exit, owner };
  }

  shutdown() {
    if (this.#shutdownPromise === null) {
      const attempt = this.#stopAllOwnedProcesses();
      this.#shutdownPromise = attempt.finally(() => {
        this.#shutdownPromise = null;
      });
    }
    return this.#shutdownPromise;
  }

  #installInterrupts() {
    for (const signal of ["SIGINT", "SIGTERM"]) {
      const handler = () => this.#interrupt(signal);
      this.#signalHandlers.set(signal, handler);
      this.signalSource.on(signal, handler);
    }
    this.#timer = setTimeout(() => this.#interrupt("编排超时"), this.timeoutMs);
  }

  #removeInterrupts() {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
    for (const [signal, handler] of this.#signalHandlers) {
      this.signalSource.off(signal, handler);
    }
    this.#signalHandlers.clear();
    this.#interruptReject = null;
  }

  #interrupt(reason) {
    if (this.#interruptedBy !== null) return;
    this.#interruptedBy = reason;
    this.#interruptReject?.(new Error(`E2E 运行被 ${reason} 中止`));
    void this.shutdown().catch(() => {});
  }

  #rememberOwnedProcess(child) {
    const key = Symbol("owned-process");
    const pid = child.pid ?? null;
    const group = this.platform !== "win32" ? pid : null;
    const tracker =
      pid === null
        ? null
        : this.platform === "win32"
          ? this.trackWindowsProcessTree(child)
          : this.trackProcessTree(pid);
    this.#ownedGroups.set(key, {
      child,
      group,
      stopping: null,
      tracked: null,
      tracker,
    });
    tracker?.start();
    return key;
  }

  #stopOwner(key) {
    const owner = this.#ownedGroups.get(key);
    if (owner === undefined) return Promise.resolve();
    if (owner.stopping === null) {
      const stopping = this.#terminateOwner(owner).then(
        () => this.#ownedGroups.delete(key),
        (error) => {
          if (owner.stopping === stopping) owner.stopping = null;
          throw error;
        },
      );
      owner.stopping = stopping;
    }
    return owner.stopping;
  }

  async #terminateOwner(owner) {
    if (this.platform === "win32") {
      if (owner.tracked === null) {
        owner.tracked = await stopTrackedWindowsProcessTree(owner.tracker);
      }
      await this.terminateWindowsTree(owner.child, this.killGraceMs, {
        knownProcesses: owner.tracked.processes,
        priorDiscoveryError: owner.tracked.discoveryError,
      });
      return;
    }
    if (owner.group === null) return;
    if (owner.tracked === null) {
      try {
        owner.tracked = await owner.tracker.stop();
      } catch (error) {
        owner.tracked = {
          discoveryError: new Error("停止运行期进程发现失败", { cause: error }),
          groups: [{ depth: 0, group: owner.group }],
        };
      }
    }
    await this.terminateGroup(owner.group, {
      killGraceMs: this.killGraceMs,
      knownGroups: owner.tracked.groups,
      priorDiscoveryError: owner.tracked.discoveryError,
      terminationGraceMs: this.terminationGraceMs,
    });
  }

  async #shutdownWithRetry() {
    try {
      await this.shutdown();
    } catch (firstError) {
      try {
        await this.shutdown();
      } catch (secondError) {
        throw combinedError("E2E 进程清理重试后仍未完成", [
          firstError,
          secondError,
        ]);
      }
    }
  }

  async #stopAllOwnedProcesses() {
    const results = await Promise.allSettled(
      [...this.#ownedGroups.keys()].map((key) => this.#stopOwner(key)),
    );
    const errors = results.flatMap((result) =>
      result.status === "rejected" ? [result.reason] : [],
    );
    if (errors.length > 0) {
      throw combinedError("一个或多个 E2E 进程组未能停止", errors);
    }
  }
}

function requirePositiveInteger(value, name) {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error(`${name} 必须是正安全整数`);
  }
}

function combinedError(message, causes) {
  return new Error(message, { cause: causes });
}
