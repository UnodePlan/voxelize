import {
  collectOwnedProcessGroups,
  mergeOwnedProcessGroups,
  parsePosixProcessTable,
  readPosixProcessTable,
} from "./posix-process-tree.mjs";

const DEFAULT_DISCOVERY_INTERVAL_MS = 1_000;

export class PosixProcessTreeTracker {
  #discoveryError = null;
  #inFlight = null;
  #knownGroups;
  #stopped = false;
  #timer = null;

  constructor(
    rootPid,
    {
      intervalMs = DEFAULT_DISCOVERY_INTERVAL_MS,
      readProcessTable: readTable = readPosixProcessTable,
    } = {},
  ) {
    requirePositiveInteger(intervalMs, "intervalMs");
    this.rootPid = rootPid;
    this.intervalMs = intervalMs;
    this.readProcessTable = readTable;
    this.#knownGroups = [{ depth: 0, group: rootPid }];
  }

  start() {
    if (this.#stopped || this.#inFlight !== null || this.#timer !== null)
      return;
    void this.refresh();
  }

  refresh() {
    if (this.#stopped) return Promise.resolve();
    this.#inFlight ??= this.#discover().finally(() => {
      this.#inFlight = null;
      this.#schedule();
    });
    return this.#inFlight;
  }

  async stop() {
    this.#stopped = true;
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = null;
    await this.#inFlight;
    return {
      discoveryError: this.#discoveryError,
      groups: this.#knownGroups,
    };
  }

  async #discover() {
    try {
      const processes = parsePosixProcessTable(await this.readProcessTable());
      const discovered = collectOwnedProcessGroups(processes, this.rootPid);
      this.#knownGroups = mergeOwnedProcessGroups(
        this.#knownGroups,
        discovered,
      );
    } catch (error) {
      this.#discoveryError ??= new Error("运行期 E2E 后代进程发现失败", {
        cause: error,
      });
    }
  }

  #schedule() {
    if (this.#stopped || this.#timer !== null) return;
    this.#timer = setTimeout(() => {
      this.#timer = null;
      void this.refresh();
    }, this.intervalMs);
    this.#timer.unref?.();
  }
}

export function trackOwnedPosixProcessTree(rootPid) {
  return new PosixProcessTreeTracker(rootPid);
}

function requirePositiveInteger(value, name) {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new Error(`${name} 必须是正安全整数`);
  }
}
