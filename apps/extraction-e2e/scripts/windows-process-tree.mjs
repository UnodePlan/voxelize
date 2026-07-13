import { isChildRunning } from "./child-process-state.mjs";
import { terminateWindowsProcessIdentity } from "./windows-process-identity.mjs";
import {
  isWindowsCreationId,
  parseWindowsProcessTable,
  readWindowsProcessTable,
  windowsProcessIdentityKey as processIdentityKey,
} from "./windows-process-snapshot.mjs";

export function collectOwnedWindowsProcesses(processes, knownProcesses) {
  const childrenByParent = new Map();
  const processByPid = new Map();
  for (const process of processes) {
    requireProcessIdentity(process);
    processByPid.set(process.pid, process);
    const children = childrenByParent.get(process.parentPid) ?? [];
    children.push(process.pid);
    childrenByParent.set(process.parentPid, children);
  }

  const identities = new Map();
  const depthByPid = new Map();
  for (const process of mergeOwnedWindowsProcesses(knownProcesses)) {
    identities.set(processIdentityKey(process), process);
    const previousDepth = depthByPid.get(process.pid);
    if (previousDepth === undefined || process.depth > previousDepth) {
      depthByPid.set(process.pid, process.depth);
    }
  }
  const pending = [...depthByPid.keys()];
  const visited = new Set();
  while (pending.length > 0) {
    const parentPid = pending.pop();
    if (parentPid === undefined || visited.has(parentPid)) continue;
    visited.add(parentPid);
    const parentDepth = depthByPid.get(parentPid);
    if (parentDepth === undefined) continue;
    const currentParent = processByPid.get(parentPid);
    if (
      currentParent === undefined ||
      !identities.has(processIdentityKey(currentParent))
    ) {
      continue;
    }
    for (const childPid of childrenByParent.get(parentPid) ?? []) {
      const child = processByPid.get(childPid);
      if (child === undefined) continue;
      const childDepth = parentDepth + 1;
      const key = processIdentityKey(child);
      if (!identities.has(key)) {
        identities.set(key, { ...child, depth: childDepth });
      }
      const previousDepth = depthByPid.get(childPid);
      if (previousDepth === undefined || childDepth > previousDepth) {
        depthByPid.set(childPid, childDepth);
      }
      pending.push(childPid);
    }
  }
  return sortOwnedProcesses(identities.values());
}

export function mergeOwnedWindowsProcesses(...collections) {
  const identities = new Map();
  for (const processes of collections) {
    for (const process of processes) {
      const { depth } = process;
      requireProcessIdentity(process);
      if (!Number.isSafeInteger(depth) || depth < 0) {
        throw new Error("进程深度必须是非负安全整数");
      }
      const key = processIdentityKey(process);
      const previous = identities.get(key);
      if (previous === undefined || depth > previous.depth) {
        identities.set(key, process);
      }
    }
  }
  return sortOwnedProcesses(identities.values());
}

export async function terminateWindowsProcessTree(
  child,
  timeoutMs,
  {
    knownProcesses = [],
    priorDiscoveryError = null,
    readProcessTable = readWindowsProcessTable,
    terminateProcess = terminateWindowsProcessIdentity,
  } = {},
) {
  requirePositiveInteger(timeoutMs, "timeoutMs");
  if (child.pid === undefined) {
    if (priorDiscoveryError !== null) throw priorDiscoveryError;
    return;
  }
  const rootPid = child.pid;
  const deadline = Date.now() + timeoutMs;
  let ownedProcesses = mergeOwnedWindowsProcesses(knownProcesses);
  const discoveryErrors =
    priorDiscoveryError === null ? [] : [priorDiscoveryError];
  try {
    const rootWasRunning = isChildRunning(child);
    const processes = parseWindowsProcessTable(await readProcessTable());
    if (
      rootWasRunning &&
      isChildRunning(child) &&
      !ownedProcesses.some(({ pid }) => pid === rootPid)
    ) {
      const root = processes.find(({ pid }) => pid === rootPid);
      if (root !== undefined) {
        ownedProcesses = mergeOwnedWindowsProcesses(ownedProcesses, [
          { ...root, depth: 0 },
        ]);
      }
    }
    ownedProcesses = mergeOwnedWindowsProcesses(
      ownedProcesses,
      collectOwnedWindowsProcesses(processes, ownedProcesses),
    );
  } catch (error) {
    discoveryErrors.push(error);
  }

  let terminationError = null;
  try {
    const rootRunning = isChildRunning(child);
    const rootIdentity = ownedProcesses.find(
      ({ depth, pid }) => depth === 0 && pid === rootPid,
    );
    const rootIdentityKey =
      rootIdentity === undefined ? null : processIdentityKey(rootIdentity);
    const targets = ownedProcesses.filter(
      (process) =>
        rootRunning || processIdentityKey(process) !== rootIdentityKey,
    );
    for (const process of targets) {
      await terminateProcess(process, { deadline });
    }
    if (isChildRunning(child)) await terminateChildHandle(child, deadline);
    await waitForOwnedProcessesStop(targets, child, deadline, readProcessTable);
  } catch (error) {
    terminationError = error;
  }

  const discoveryError =
    discoveryErrors.length === 0
      ? null
      : new Error("无法确认 E2E Windows 根进程的完整后代闭包", {
          cause: discoveryErrors,
        });
  if (discoveryError !== null && terminationError !== null) {
    throw new Error("Windows 进程树发现与清理均未完成", {
      cause: [discoveryError, terminationError],
    });
  }
  if (discoveryError !== null) throw discoveryError;
  if (terminationError !== null) throw terminationError;
}

async function waitForOwnedProcessesStop(
  targets,
  child,
  deadline,
  readProcessTable,
) {
  let survivors = await findSurvivors(targets, child, readProcessTable);
  while (survivors.length > 0) {
    await delay(Math.min(50, remainingMilliseconds(deadline)));
    survivors = await findSurvivors(targets, child, readProcessTable);
  }
  if (survivors.length > 0) {
    throw new Error(
      `E2E Windows 进程 ${survivors.join(", ")} 未能在期限内退出`,
    );
  }
}

async function findSurvivors(targets, child, readProcessTable) {
  const rootPid = child.pid;
  const runningProcesses = new Set(
    parseWindowsProcessTable(await readProcessTable()).map(processIdentityKey),
  );
  return targets.flatMap((process) => {
    if (process.pid === rootPid) {
      return isChildRunning(child) ? [process.pid] : [];
    }
    return runningProcesses.has(processIdentityKey(process))
      ? [process.pid]
      : [];
  });
}

async function terminateChildHandle(child, deadline) {
  remainingMilliseconds(deadline);
  let signalled;
  try {
    signalled = child.kill();
  } catch (error) {
    throw new Error(`无法通过自有句柄终止 Windows 根进程 ${child.pid}`, {
      cause: error,
    });
  }
  if (!signalled && isChildRunning(child)) {
    throw new Error(`Windows 根进程 ${child.pid} 拒绝自有句柄终止请求`);
  }
  while (isChildRunning(child)) {
    await delay(Math.min(50, remainingMilliseconds(deadline)));
  }
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function sortOwnedProcesses(processes) {
  return [...processes].sort(
    (left, right) =>
      right.depth - left.depth ||
      right.pid - left.pid ||
      right.creationId.localeCompare(left.creationId),
  );
}

function requireProcessIdentity(process) {
  requireProcessId(process.pid, "pid");
  if (!isWindowsCreationId(process.creationId)) {
    throw new Error("creationId 必须是非空且不含空白的字符串");
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

function remainingMilliseconds(deadline) {
  const remaining = deadline - Date.now();
  if (remaining <= 0) throw new Error("Windows 进程清理超过截止时间");
  return Math.ceil(remaining);
}
