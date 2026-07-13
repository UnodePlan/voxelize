import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { PosixProcessTreeTracker } from "./posix-process-tracker.mjs";
import {
  collectOwnedProcessGroups,
  parsePosixProcessTable,
  terminateOwnedPosixProcessTree,
} from "./posix-process-tree.mjs";
import { ProcessSupervisor } from "./process-supervisor.mjs";
import { terminateWindowsProcessIdentity } from "./windows-process-identity.mjs";
import { WindowsProcessTreeTracker } from "./windows-process-tracker.mjs";
import {
  collectOwnedWindowsProcesses,
  terminateWindowsProcessTree,
} from "./windows-process-tree.mjs";

test("正常退出后只核对并释放自己启动的进程组", async () => {
  const signals = new EventEmitter();
  const terminated = [];
  const child = fakeChild(101);
  const supervisor = createSupervisor({
    child,
    signalSource: signals,
    terminateGroup: async (group) => terminated.push(group),
  });

  await supervisor.supervise(async (owner) => {
    queueMicrotask(() => child.finish(0, null));
    await owner.runCommand("纯测试命令", "fake", []);
  });

  assert.deepEqual(terminated, [101]);
  assert.equal(signals.listenerCount("SIGINT"), 0);
  assert.equal(signals.listenerCount("SIGTERM"), 0);
});

test("SIGTERM 会中止编排并有界清理当前自有进程组", async () => {
  const signals = new EventEmitter();
  const terminated = [];
  const child = fakeChild(202);
  const supervisor = createSupervisor({
    child,
    signalSource: signals,
    terminateGroup: async (group) => {
      terminated.push(group);
      child.finish(null, "SIGTERM");
    },
  });

  await assert.rejects(
    supervisor.supervise(async (owner) => {
      queueMicrotask(() => signals.emit("SIGTERM"));
      await owner.runCommand("阻塞命令", "fake", []);
    }),
    /SIGTERM/u,
  );

  assert.equal(supervisor.interruptedBy, "SIGTERM");
  assert.deepEqual(terminated, [202]);
});

test("总超时会终止当前命令且禁止继续启动下一条命令", async () => {
  const child = fakeChild(303);
  let spawnCount = 0;
  const supervisor = new ProcessSupervisor({
    cwd: "/tmp",
    environment: {},
    logPrefix: "test",
    platform: "linux",
    signalSource: new EventEmitter(),
    spawn: () => {
      spawnCount += 1;
      return child;
    },
    trackProcessTree: (group) => fakeTracker(group),
    terminateGroup: async () => child.finish(null, "SIGTERM"),
    timeoutMs: 10,
  });

  await assert.rejects(
    supervisor.supervise(async (owner) => {
      await owner.runCommand("超时命令", "fake", []);
      await owner.runCommand("不应启动", "fake", []);
    }),
    /编排超时/u,
  );

  assert.equal(supervisor.interruptedBy, "编排超时");
  assert.equal(spawnCount, 1);
});

test("首次清理失败会保留 owner 供总收尾重试", async () => {
  const child = fakeChild(350);
  let attempts = 0;
  const supervisor = createSupervisor({
    child,
    terminateGroup: async () => {
      attempts += 1;
      if (attempts === 1) throw new Error("first cleanup failed");
    },
  });

  await assert.rejects(
    supervisor.supervise(async (owner) => {
      queueMicrotask(() => child.finish(0, null));
      await owner.runCommand("清理重试命令", "fake", []);
    }),
    /first cleanup failed/u,
  );

  assert.equal(attempts, 2);
});

test("后台命令使用独立环境并由监督器总收尾清理", async () => {
  const child = fakeChild(360);
  const environments = [];
  const terminated = [];
  const supervisor = new ProcessSupervisor({
    cwd: "/tmp",
    environment: { BASE: "base" },
    logPrefix: "test",
    platform: "linux",
    signalSource: new EventEmitter(),
    spawn: (_command, _args, options) => {
      environments.push(options.env);
      return child;
    },
    trackProcessTree: (group) => fakeTracker(group),
    terminateGroup: async (group) => {
      terminated.push(group);
      child.finish(null, "SIGTERM");
    },
    timeoutMs: 1_000,
  });

  await supervisor.supervise((owner) => {
    assert.equal(
      owner.startCommand("后台服务", "fake", [], {
        environment: { ONLY: "override" },
      }),
      child,
    );
  });

  assert.deepEqual(environments, [{ ONLY: "override" }]);
  assert.deepEqual(terminated, [360]);
});

test("Windows 正常有限命令无历史后代时成功收尾", async () => {
  const signals = new EventEmitter();
  const child = fakeChild(401);
  const supervisor = new ProcessSupervisor({
    cwd: "/tmp",
    environment: {},
    logPrefix: "test",
    platform: "win32",
    signalSource: signals,
    spawn: () => child,
    timeoutMs: 1_000,
    trackWindowsProcessTree: (ownedChild) =>
      new WindowsProcessTreeTracker(ownedChild, {
        intervalMs: 60_000,
        readProcessTable: async () =>
          ownedChild.exitCode === null && ownedChild.signalCode === null
            ? "401 1 4010"
            : "",
        setTimer: () => ({ unref() {} }),
      }),
    terminateWindowsTree: (ownedChild, timeoutMs, ownership) =>
      terminateWindowsProcessTree(ownedChild, timeoutMs, {
        ...ownership,
        readProcessTable: async () => "",
      }),
  });

  await supervisor.supervise(async (owner) => {
    setImmediate(() => child.finish(0, null));
    await owner.runCommand("Windows 有限命令", "fake", []);
  });

  assert.equal(signals.listenerCount("SIGINT"), 0);
  assert.equal(signals.listenerCount("SIGTERM"), 0);
});

test("Windows 跟踪器跨根退出保留后代闭包并排除无关 PID", async () => {
  const child = fakeChild(401);
  let processTable = `
    410 401 4100
    401 1 4010
    900 1 9000
  `;
  const scheduled = [];
  const tracker = new WindowsProcessTreeTracker(child, {
    intervalMs: 10,
    readProcessTable: async () => processTable,
    setTimer: (callback) => {
      scheduled.push(callback);
      return { unref() {} };
    },
  });
  tracker.start();
  await tracker.refresh();

  processTable = `
    410 401 4100
    411 410 4110
    900 1 9000
    901 900 9010
  `;
  child.finish(0, null);
  scheduled.shift()?.();
  await tracker.refresh();
  const tracked = await tracker.stop();

  assert.equal(tracked.discoveryError, null);
  assert.deepEqual(tracked.processes, [
    { creationId: "4110", depth: 2, parentPid: 410, pid: 411 },
    { creationId: "4100", depth: 1, parentPid: 401, pid: 410 },
    { creationId: "4010", depth: 0, parentPid: 1, pid: 401 },
  ]);
});

test("Windows 首帧期间根退出时拒绝复用 PID 污染并报告发现失败", async () => {
  const child = fakeChild(405);
  let processTable = "405 1 reused\n406 405 descendant";
  let firstSnapshot = true;
  const tracker = new WindowsProcessTreeTracker(child, {
    clearTimer() {},
    intervalMs: 60_000,
    readProcessTable: async () => {
      if (firstSnapshot) {
        firstSnapshot = false;
        child.finish(0, null);
      }
      return processTable;
    },
    setTimer: () => ({ unref() {} }),
  });
  tracker.start();
  await tracker.refresh();
  processTable = "405 1 later-reuse\n407 405 unrelated";
  const tracked = await tracker.stop();

  assert.match(tracked.discoveryError?.message ?? "", /可信观察/u);
  assert.deepEqual(tracked.processes, []);
});

test("Windows 历史父身份消失或复用时不扩展其 PPID 子树", () => {
  const known = [
    { creationId: "root", depth: 0, parentPid: 1, pid: 401 },
    { creationId: "owned", depth: 1, parentPid: 401, pid: 410 },
  ];
  const collected = collectOwnedWindowsProcesses(
    [
      { creationId: "reused", parentPid: 1, pid: 410 },
      { creationId: "unrelated", parentPid: 410, pid: 999 },
    ],
    known,
  );

  assert.equal(
    collected.some(({ pid }) => pid === 999),
    false,
  );
});

test("Windows tracker.start 同步失败后 owner 仍由监督器清理", async () => {
  const child = fakeChild(406);
  let cleanupCount = 0;
  const supervisor = new ProcessSupervisor({
    cwd: "/tmp",
    environment: {},
    logPrefix: "test",
    platform: "win32",
    signalSource: new EventEmitter(),
    spawn: () => child,
    timeoutMs: 1_000,
    trackWindowsProcessTree: () => ({
      start() {
        throw new Error("tracker start failed");
      },
      async stop() {
        return { discoveryError: null, processes: [] };
      },
    }),
    terminateWindowsTree: async (ownedChild) => {
      cleanupCount += 1;
      ownedChild.finish(null, "SIGTERM");
    },
  });

  await assert.rejects(
    supervisor.supervise((owner) =>
      owner.startCommand("tracker 启动失败命令", "fake", []),
    ),
    /tracker start failed/u,
  );
  assert.equal(cleanupCount, 1);
});

test("Windows 根先退出后按子优先清理复用 root PID 的自有后代", async () => {
  const child = fakeChild(420);
  child.finish(0, null);
  const parents = new Map([
    [421, 420],
    [420, 421],
    [900, 1],
  ]);
  const creationIds = new Map([
    [421, "4210"],
    [420, "420-reused"],
    [900, "9000"],
  ]);
  const running = new Set(parents.keys());
  const terminated = [];

  await terminateWindowsProcessTree(child, 100, {
    knownProcesses: [
      { creationId: "4200", depth: 0, parentPid: 1, pid: 420 },
      { creationId: "4210", depth: 1, parentPid: 420, pid: 421 },
    ],
    readProcessTable: async () =>
      windowsProcessTable(running, parents, creationIds),
    terminateProcess: async (process) => {
      terminated.push(process);
      running.delete(process.pid);
      return "terminated";
    },
  });

  assert.deepEqual(
    terminated.map(({ creationId, pid }) => ({ creationId, pid })),
    [
      { creationId: "420-reused", pid: 420 },
      { creationId: "4210", pid: 421 },
    ],
  );
  assert.equal(running.has(900), true);
});

test("Windows 快照后 PID 被复用时原子终止器不杀无关进程", async () => {
  const child = fakeChild(430);
  child.finish(0, null);
  let snapshotCount = 0;
  const invocations = [];

  await terminateWindowsProcessTree(child, 10, {
    knownProcesses: [{ creationId: "old", depth: 1, parentPid: 430, pid: 431 }],
    readProcessTable: async () =>
      snapshotCount++ === 0 ? "431 430 old" : "431 1 reused",
    terminateProcess: (process) =>
      terminateWindowsProcessIdentity(process, {
        execFileProcess: fakePowerShell("reused", invocations),
      }),
  });

  assert.equal(invocations.length, 1);
  assert.deepEqual(invocations[0].args.slice(-4), [
    "-OwnedPid",
    "431",
    "-ExpectedCreationId",
    "old",
  ]);
});

test("Windows 原子身份终止错误会失败闭合", async () => {
  const child = fakeChild(402);

  await assert.rejects(
    terminateWindowsProcessTree(child, 10, {
      knownProcesses: [
        { creationId: "4020", depth: 0, parentPid: 1, pid: 402 },
      ],
      readProcessTable: async () => "402 1 4020",
      terminateProcess: async () => {
        throw new Error("access denied");
      },
    }),
    /access denied/u,
  );
});

test("Windows 未观察根只通过 ChildProcess 句柄终止且失败可见", async () => {
  const child = fakeChild(403);
  child.kill = () => {
    throw new Error("handle denied");
  };

  await assert.rejects(
    terminateWindowsProcessTree(child, 10, {
      readProcessTable: async () => "",
    }),
    /自有句柄/u,
  );
});

test("Windows 已观察根也通过 creationId 原子终止", async () => {
  const child = fakeChild(404);
  const terminated = [];

  await terminateWindowsProcessTree(child, 10, {
    knownProcesses: [{ creationId: "4040", depth: 0, parentPid: 1, pid: 404 }],
    readProcessTable: async () =>
      child.exitCode === null && child.signalCode === null ? "404 1 4040" : "",
    terminateProcess: async (process) => {
      terminated.push(process);
      child.finish(null, "SIGTERM");
      return "terminated";
    },
  });
  assert.equal(child.signalCode, "SIGTERM");
  assert.deepEqual(
    terminated.map(({ creationId, pid }) => ({ creationId, pid })),
    [{ creationId: "4040", pid: 404 }],
  );
});

test("Windows 最终快照失败仍尝试原子清理全部历史后代并报告失败", async () => {
  const child = fakeChild(440);
  child.finish(0, null);
  const attempted = [];

  await assert.rejects(
    terminateWindowsProcessTree(child, 20, {
      knownProcesses: [
        { creationId: "4410", depth: 1, parentPid: 440, pid: 441 },
      ],
      readProcessTable: async () => {
        throw new Error("snapshot failed");
      },
      terminateProcess: async (process) => {
        attempted.push(process.pid);
        return "gone";
      },
    }),
    /发现与清理均未完成/u,
  );

  assert.deepEqual(attempted, [441]);
});

test("PowerShell 原子终止器接受 gone、reused 与 terminated", async () => {
  for (const status of ["gone", "reused", "terminated"]) {
    const invocations = [];
    const result = await terminateWindowsProcessIdentity(
      { creationId: "4510", pid: 451 },
      { execFileProcess: fakePowerShell(status, invocations) },
    );

    assert.equal(result, status);
    assert.equal(invocations[0].command, "powershell.exe");
    assert.equal(invocations[0].args.includes("-Command"), false);
    assert.match(invocations[0].args[3], /terminate-windows-process\.ps1$/u);
    assert.deepEqual(invocations[0].args.slice(-4), [
      "-OwnedPid",
      "451",
      "-ExpectedCreationId",
      "4510",
    ]);
  }
});

test("PowerShell 原子终止器查询、权限或终止错误会失败闭合", async () => {
  const invocations = [];
  await assert.rejects(
    terminateWindowsProcessIdentity(
      { creationId: "4520", pid: 452 },
      {
        execFileProcess: fakePowerShell(
          new Error("access denied"),
          invocations,
        ),
      },
    ),
    /无法精确终止/u,
  );
  assert.equal(invocations.length, 1);
});

test("PowerShell 原子终止 helper 挂起时按同一 deadline 终止 helper", async () => {
  let helperKilled = false;
  await assert.rejects(
    terminateWindowsProcessIdentity(
      { creationId: "4530", pid: 453 },
      {
        deadline: Date.now() + 5,
        execFileProcess: () => ({
          kill() {
            helperKilled = true;
          },
        }),
      },
    ),
    /超时/u,
  );
  assert.equal(helperKilled, true);
});

test("Windows 未观察根的 ChildProcess 句柄终止也受绝对 deadline 限制", async () => {
  const child = fakeChild(454);
  let killCount = 0;
  child.kill = () => {
    killCount += 1;
    return true;
  };

  await assert.rejects(
    terminateWindowsProcessTree(child, 5, {
      readProcessTable: async () => "",
    }),
    /截止时间/u,
  );
  assert.equal(killCount, 1);
});

test("PowerShell 脚本在同一 Process 对象上核验 StartTime 并 Kill", async () => {
  const script = await readFile(
    new URL("./terminate-windows-process.ps1", import.meta.url),
    "utf8",
  );

  assert.match(script, /\$process = Get-Process/u);
  assert.match(script, /\$process\.StartTime/u);
  assert.match(script, /\$process\.Kill\(\)/u);
  assert.doesNotMatch(script, /Invoke-CimMethod|taskkill/u);
});

test("POSIX 后代闭包包含 detached 子组且排除无关用户进程", () => {
  const processes = parsePosixProcessTable(`
    100 1 100
    101 100 100
    102 101 100
    200 102 200
    201 200 200
    300 201 300
    900 1 900
    901 900 900
  `);

  assert.deepEqual(collectOwnedProcessGroups(processes, 100), [
    { depth: 5, group: 300 },
    { depth: 3, group: 200 },
    { depth: 0, group: 100 },
  ]);
});

test("根退出并 reparent 后仍使用运行期已确认的 detached 组", async () => {
  let processTable = `
    100 1 100
    110 100 100
    200 110 200
    201 200 200
    900 1 900
  `;
  const tracker = new PosixProcessTreeTracker(100, {
    intervalMs: 60_000,
    readProcessTable: async () => processTable,
  });
  await tracker.refresh();

  processTable = `
    200 1 200
    201 200 200
    900 1 900
  `;
  const tracked = await tracker.stop();
  const sent = [];
  const running = new Set([200, 900]);
  const killProcess = createKillProcess(running, sent);
  await terminateOwnedPosixProcessTree(100, {
    killGraceMs: 2,
    killProcess,
    knownGroups: tracked.groups,
    priorDiscoveryError: tracked.discoveryError,
    readProcessTable: async () => processTable,
    terminationGraceMs: 5,
    waitForStop: async (groups) => groups.filter((group) => running.has(group)),
  });

  assert.deepEqual(sent, [{ group: 200, signal: "SIGTERM" }]);
  assert.equal(running.has(900), true);
});

test("POSIX 清理按子组到父组 TERM，仅对存活自有组升级 KILL", async () => {
  const sent = [];
  const running = new Set([100, 200, 300, 900]);
  const killProcess = (pid, signal) => {
    const group = Math.abs(pid);
    if (signal === 0) {
      if (running.has(group)) return;
      const error = new Error("not found");
      error.code = "ESRCH";
      throw error;
    }
    sent.push({ group, signal });
    if (signal === "SIGKILL" || group !== 200) running.delete(group);
  };

  await terminateOwnedPosixProcessTree(100, {
    killGraceMs: 2,
    killProcess,
    terminationGraceMs: 5,
    readProcessTable: async () => `
      100 1 100
      110 100 100
      200 110 200
      300 200 300
      900 1 900
    `,
    waitForStop: async (groups) => groups.filter((group) => running.has(group)),
  });

  assert.deepEqual(sent, [
    { group: 300, signal: "SIGTERM" },
    { group: 200, signal: "SIGTERM" },
    { group: 100, signal: "SIGTERM" },
    { group: 200, signal: "SIGKILL" },
  ]);
  assert.equal(running.has(900), true);
});

test("POSIX 进程表发现失败时只清理已知根组并报告失败", async () => {
  const sent = [];
  const running = new Set([100, 900]);
  const killProcess = createKillProcess(running, sent);

  await assert.rejects(
    terminateOwnedPosixProcessTree(100, {
      killGraceMs: 2,
      killProcess,
      readProcessTable: async () => {
        throw new Error("ps failed");
      },
      terminationGraceMs: 5,
      waitForStop: async (groups) =>
        groups.filter((group) => running.has(group)),
    }),
    /完整后代闭包/u,
  );

  assert.deepEqual(sent, [{ group: 100, signal: "SIGTERM" }]);
  assert.equal(running.has(900), true);
});

function createSupervisor({ child, signalSource, terminateGroup }) {
  return new ProcessSupervisor({
    cwd: "/tmp",
    environment: {},
    logPrefix: "test",
    platform: "linux",
    signalSource,
    spawn: () => child,
    trackProcessTree: (group) => fakeTracker(group),
    terminateGroup,
    timeoutMs: 1_000,
  });
}

function fakeTracker(group) {
  return {
    start() {},
    async stop() {
      return {
        discoveryError: null,
        groups: [{ depth: 0, group }],
      };
    },
  };
}

function createKillProcess(running, sent) {
  return (pid, signal) => {
    const group = Math.abs(pid);
    if (signal === 0) {
      if (running.has(group)) return;
      const error = new Error("not found");
      error.code = "ESRCH";
      throw error;
    }
    sent.push({ group, signal });
    running.delete(group);
  };
}

function windowsProcessTable(running, parents, creationIds) {
  return [...running]
    .map((pid) => `${pid} ${parents.get(pid)} ${creationIds.get(pid)}`)
    .join("\n");
}

function fakePowerShell(result, invocations) {
  return (command, args, options, callback) => {
    const helper = { kill() {} };
    invocations.push({ args, command, helper, options });
    queueMicrotask(() => {
      if (result instanceof Error) callback(result, "", "");
      else callback(null, `${result}\r\n`, "");
    });
    return helper;
  };
}

function fakeChild(pid) {
  const child = new EventEmitter();
  child.pid = pid;
  child.exitCode = null;
  child.signalCode = null;
  child.finish = (code, signal) => {
    if (child.exitCode !== null || child.signalCode !== null) return;
    child.exitCode = code;
    child.signalCode = signal;
    child.emit("exit", code, signal);
  };
  return child;
}
