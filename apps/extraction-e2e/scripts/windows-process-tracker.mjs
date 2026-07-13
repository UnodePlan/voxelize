import { isChildRunning } from "./child-process-state.mjs";
import {
  parseWindowsProcessTable,
  readWindowsProcessTable,
} from "./windows-process-snapshot.mjs";
import {
  collectOwnedWindowsProcesses,
  mergeOwnedWindowsProcesses,
} from "./windows-process-tree.mjs";

const DEFAULT_DISCOVERY_INTERVAL_MS = 250;

export class WindowsProcessTreeTracker {
  #discoveryError = null;
  #inFlight = null;
  #knownProcesses;
  #rootObserved = false;
  #stopPromise = null;
  #stopping = false;
  #timer = null;

  constructor(
    child,
    {
      clearTimer = clearTimeout,
      intervalMs = DEFAULT_DISCOVERY_INTERVAL_MS,
      readProcessTable: readTable = readWindowsProcessTable,
      setTimer = setTimeout,
    } = {},
  ) {
    const rootPid = child.pid;
    requireProcessId(rootPid, "rootPid");
    requirePositiveInteger(intervalMs, "intervalMs");
    this.child = child;
    this.rootPid = rootPid;
    this.intervalMs = intervalMs;
    this.readProcessTable = readTable;
    this.clearTimer = clearTimer;
    this.setTimer = setTimer;
    this.#knownProcesses = [];
  }

  start() {
    if (this.#stopping || this.#inFlight !== null || this.#timer !== null)
      return;
    void this.refresh();
  }

  refresh() {
    if (this.#stopping) return Promise.resolve();
    this.#inFlight ??= this.#discover().finally(() => {
      this.#inFlight = null;
      this.#schedule();
    });
    return this.#inFlight;
  }

  stop() {
    this.#stopPromise ??= this.#finishStop();
    return this.#stopPromise;
  }

  async #finishStop() {
    this.#stopping = true;
    if (this.#timer !== null) this.clearTimer(this.#timer);
    this.#timer = null;
    await this.#inFlight;
    await this.#discover();
    if (!this.#rootObserved) {
      this.#discoveryError ??= new Error(
        `未能在 Windows 根进程 ${this.rootPid} 退出前可信观察其身份`,
      );
    }
    return {
      discoveryError: this.#discoveryError,
      processes: this.#knownProcesses,
    };
  }

  async #discover() {
    try {
      const rootWasRunning = isChildRunning(this.child);
      const processes = parseWindowsProcessTable(await this.readProcessTable());
      if (this.#knownProcesses.length === 0) {
        const root = processes.find(({ pid }) => pid === this.rootPid);
        if (
          root === undefined ||
          !rootWasRunning ||
          !isChildRunning(this.child)
        ) {
          return;
        }
        this.#knownProcesses = [{ ...root, depth: 0 }];
        this.#rootObserved = true;
      }
      this.#knownProcesses = mergeOwnedWindowsProcesses(
        this.#knownProcesses,
        collectOwnedWindowsProcesses(processes, this.#knownProcesses),
      );
    } catch (error) {
      this.#discoveryError ??= new Error(
        "运行期 E2E Windows 后代进程发现失败",
        {
          cause: error,
        },
      );
    }
  }

  #schedule() {
    if (this.#stopping || this.#timer !== null) return;
    this.#timer = this.setTimer(() => {
      this.#timer = null;
      void this.refresh();
    }, this.intervalMs);
    this.#timer?.unref?.();
  }
}

export function trackOwnedWindowsProcessTree(child) {
  return new WindowsProcessTreeTracker(child);
}

export async function stopTrackedWindowsProcessTree(tracker) {
  if (tracker === null) return { discoveryError: null, processes: [] };
  try {
    return await tracker.stop();
  } catch (error) {
    return {
      discoveryError: new Error("停止运行期 Windows 进程发现失败", {
        cause: error,
      }),
      processes: [],
    };
  }
}

function requireProcessId(value, name) {
  if (!Number.isSafeInteger(value) || value <= 1) {
    throw new Error(`${name} 必须是大于 1 的安全整数`);
  }
}

function requirePositiveInteger(value, name) {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error(`${name} 必须是正安全整数`);
  }
}
