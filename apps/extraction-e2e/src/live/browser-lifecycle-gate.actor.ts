import { describe, expect, test } from "vitest";

import {
  baselineLifecycleErrors,
  inputIsolationErrors,
  matchLifecycleErrors,
  releasedLifecycleErrors,
  type BrowserLifecycleProfile,
} from "./browser-lifecycle-gate";

function lobbyProfile(): BrowserLifecycleProfile {
  return {
    activeGameSocketCount: 1,
    closedGameSocketCount: 0,
    gameSocketCount: 1,
    liveWorkerCount: 1,
    webglCanvasCount: 1,
    webglContextCount: 1,
    webglContextLostCount: 0,
    webglContextRestoredCount: 0,
    workerRoles: {
      light: 0,
      mesh: 0,
      other: 0,
      protocol: 1,
      urgentMesh: 0,
    },
  };
}

function matchProfile(): BrowserLifecycleProfile {
  return {
    ...lobbyProfile(),
    liveWorkerCount: 4,
    workerRoles: {
      light: 1,
      mesh: 1,
      other: 0,
      protocol: 1,
      urgentMesh: 1,
    },
  };
}

describe("同页跨局资源门禁规则", () => {
  test("接受单 socket、单 protocol Worker 和单 WebGL context 大厅基线", () => {
    expect(baselineLifecycleErrors(lobbyProfile())).toEqual([]);
  });

  test("接受稳定的比赛 Worker 角色并在离场后回到基线", () => {
    const baseline = lobbyProfile();
    const active = matchProfile();

    expect(
      matchLifecycleErrors(active, baseline, 1, active.workerRoles),
    ).toEqual([]);
    const released = {
      ...baseline,
      activeGameSocketCount: 0,
      closedGameSocketCount: 1,
    };
    expect(releasedLifecycleErrors(released, baseline, 1)).toEqual([]);
  });

  test("拒绝下一局增加 Worker、socket 或 WebGL context", () => {
    const baseline = lobbyProfile();
    const active = matchProfile();
    const grown = {
      ...active,
      gameSocketCount: 2,
      closedGameSocketCount: 0,
      liveWorkerCount: 5,
      webglContextCount: 2,
      workerRoles: { ...active.workerRoles, mesh: 2 },
    };

    expect(
      matchLifecycleErrors(grown, baseline, 2, active.workerRoles),
    ).toEqual(
      expect.arrayContaining([
        "active round game socket lifecycle is inconsistent",
        "WebGL context count changed between rounds",
        "active World worker counts grew between rounds",
      ]),
    );
  });

  test("拒绝退局后残留 Worker 或继续发送输入", () => {
    const baseline = lobbyProfile();
    const leaked = matchProfile();

    expect(releasedLifecycleErrors(leaked, baseline, 1)).toEqual(
      expect.arrayContaining([
        "released World workers did not return to the lobby baseline",
        "live Worker count did not return to the lobby baseline",
      ]),
    );
    expect(
      inputIsolationErrors({
        outgoingWorldFrames: ["PEER", "METHOD:pvp:v1:attack"],
        pointerLocked: true,
      }),
    ).toHaveLength(2);
  });
});
