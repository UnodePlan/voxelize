import { execFile } from "node:child_process";

const PROCESS_SNAPSHOT_TIMEOUT_MS = 2_000;
const DEFAULT_TERMINATION_GRACE_MS = 5_000;
const DEFAULT_KILL_GRACE_MS = 2_000;

export function parsePosixProcessTable(output) {
  if (typeof output !== "string") throw new TypeError("进程表输出必须是字符串");
  const processes = [];
  for (const [index, line] of output.split(/\r?\n/u).entries()) {
    const value = line.trim();
    if (value === "") continue;
    const columns = value.split(/\s+/u).map(Number);
    if (
      columns.length !== 3 ||
      !columns.every(Number.isSafeInteger) ||
      columns[0] <= 0 ||
      columns[1] < 0 ||
      columns[2] <= 0
    ) {
      throw new Error(`无法解析进程表第 ${index + 1} 行`);
    }
    const [pid, parentPid, group] = columns;
    processes.push({ group, parentPid, pid });
  }
  return processes;
}

export function collectOwnedProcessGroups(processes, rootPid) {
  requireProcessId(rootPid, "rootPid");
  const childrenByParent = new Map();
  const processByPid = new Map();
  for (const process of processes) {
    processByPid.set(process.pid, process);
    const children = childrenByParent.get(process.parentPid) ?? [];
    children.push(process.pid);
    childrenByParent.set(process.parentPid, children);
  }

  const root = processByPid.get(rootPid);
  if (root === undefined) return [];
  if (root.group !== rootPid) {
    throw new Error(`自有根进程 ${rootPid} 未处于预期的独立进程组`);
  }

  const depthByGroup = new Map([[rootPid, 0]]);
  const visited = new Set();
  const pending = [{ depth: 0, pid: rootPid }];
  while (pending.length > 0) {
    const current = pending.pop();
    if (current === undefined || visited.has(current.pid)) continue;
    visited.add(current.pid);
    const process = processByPid.get(current.pid);
    if (process !== undefined && process.group > 1) {
      const previousDepth = depthByGroup.get(process.group);
      if (previousDepth === undefined || current.depth < previousDepth) {
        depthByGroup.set(process.group, current.depth);
      }
    }
    for (const childPid of childrenByParent.get(current.pid) ?? []) {
      pending.push({ depth: current.depth + 1, pid: childPid });
    }
  }

  return [...depthByGroup]
    .map(([group, depth]) => ({ depth, group }))
    .sort(
      (left, right) => right.depth - left.depth || right.group - left.group,
    );
}

export function mergeOwnedProcessGroups(...collections) {
  const depthByGroup = new Map();
  for (const groups of collections) {
    for (const { depth, group } of groups) {
      requireProcessId(group, "group");
      if (!Number.isSafeInteger(depth) || depth < 0) {
        throw new Error("进程组深度必须是非负安全整数");
      }
      const previousDepth = depthByGroup.get(group);
      if (previousDepth === undefined || depth < previousDepth) {
        depthByGroup.set(group, depth);
      }
    }
  }
  return [...depthByGroup]
    .map(([group, depth]) => ({ depth, group }))
    .sort(
      (left, right) => right.depth - left.depth || right.group - left.group,
    );
}

export async function terminateOwnedPosixProcessTree(
  rootPid,
  {
    killGraceMs = DEFAULT_KILL_GRACE_MS,
    killProcess = process.kill,
    knownGroups = [{ depth: 0, group: rootPid }],
    priorDiscoveryError = null,
    readProcessTable = readPosixProcessTable,
    terminationGraceMs = DEFAULT_TERMINATION_GRACE_MS,
    waitForStop = waitForProcessGroupsStop,
  } = {},
) {
  requireProcessId(rootPid, "rootPid");
  requirePositiveInteger(terminationGraceMs, "terminationGraceMs");
  requirePositiveInteger(killGraceMs, "killGraceMs");

  const discoveryErrors =
    priorDiscoveryError === null ? [] : [priorDiscoveryError];
  let ownedGroups = mergeOwnedProcessGroups(
    [{ depth: 0, group: rootPid }],
    knownGroups,
  );
  try {
    const processes = parsePosixProcessTable(await readProcessTable());
    ownedGroups = mergeOwnedProcessGroups(
      ownedGroups,
      collectOwnedProcessGroups(processes, rootPid),
    );
  } catch (error) {
    discoveryErrors.push(error);
  }
  const discoveryError =
    discoveryErrors.length === 0
      ? null
      : new Error("无法确认 E2E 根进程的完整后代闭包", {
          cause: discoveryErrors,
        });

  let terminationError = null;
  try {
    await terminateProcessGroups(
      ownedGroups.map(({ group }) => group),
      {
        killGraceMs,
        killProcess,
        terminationGraceMs,
        waitForStop,
      },
    );
  } catch (error) {
    terminationError = error;
  }
  if (discoveryError !== null && terminationError !== null) {
    throw new Error("进程树发现与清理均未完成", {
      cause: [discoveryError, terminationError],
    });
  }
  if (discoveryError !== null) throw discoveryError;
  if (terminationError !== null) throw terminationError;
}

async function terminateProcessGroups(
  groups,
  { killGraceMs, killProcess, terminationGraceMs, waitForStop },
) {
  const running = groups.filter((group) =>
    isProcessGroupRunning(group, killProcess),
  );
  for (const group of running)
    sendProcessGroupSignal(group, "SIGTERM", killProcess);
  let survivors = await waitForStop(running, terminationGraceMs, killProcess);
  for (const group of survivors)
    sendProcessGroupSignal(group, "SIGKILL", killProcess);
  survivors = await waitForStop(survivors, killGraceMs, killProcess);
  if (survivors.length > 0) {
    throw new Error(`E2E 进程组 ${survivors.join(", ")} 未能在期限内退出`);
  }
}

export function readPosixProcessTable() {
  return new Promise((resolve, reject) => {
    execFile(
      "ps",
      ["-axo", "pid=,ppid=,pgid="],
      {
        encoding: "utf8",
        maxBuffer: 16 * 1024 * 1024,
        timeout: PROCESS_SNAPSHOT_TIMEOUT_MS,
      },
      (error, stdout) => {
        if (error !== null) reject(error);
        else resolve(stdout);
      },
    );
  });
}

async function waitForProcessGroupsStop(groups, timeoutMs, killProcess) {
  const deadline = Date.now() + timeoutMs;
  let running = groups.filter((group) =>
    isProcessGroupRunning(group, killProcess),
  );
  while (running.length > 0 && Date.now() < deadline) {
    await delay(50);
    running = groups.filter((group) =>
      isProcessGroupRunning(group, killProcess),
    );
  }
  return running;
}

function isProcessGroupRunning(group, killProcess) {
  try {
    killProcess(-group, 0);
    return true;
  } catch (error) {
    if (error?.code === "ESRCH") return false;
    if (error?.code === "EPERM") return true;
    throw error;
  }
}

function sendProcessGroupSignal(group, signal, killProcess) {
  try {
    killProcess(-group, signal);
  } catch (error) {
    if (error?.code !== "ESRCH") throw error;
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

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
