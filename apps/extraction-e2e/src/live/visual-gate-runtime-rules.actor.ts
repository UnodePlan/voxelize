import { describe, expect, test } from "vitest";

import type { RuntimeDiagnosticsSnapshot } from "./visual-gate-diagnostics";
import {
  hasDeterministicRuntimeFailure,
  runtimeEvidenceErrors,
} from "./visual-gate-runtime-rules";

function validSnapshot(): RuntimeDiagnosticsSnapshot {
  const workers = [
    { evaluable: true, name: "mesh-worker-0", url: "mesh.js" },
    {
      evaluable: true,
      name: "mesh-worker-urgent-0",
      url: "urgent.js",
    },
    { evaluable: true, name: "light-worker-0", url: "light.js" },
  ];
  return {
    activeGameSocketCount: 1,
    closedGameSocketCount: 0,
    errors: [],
    evaluableWorkerCount: workers.length,
    gameSocketCount: 1,
    liveWorkerCount: workers.length,
    meshResponseCount: 1,
    nonEmptyMeshResponseCount: 1,
    observedWorkers: workers.map(({ name, url }) => ({ name, url })),
    observedWorkerUrls: workers.map(({ url }) => url),
    webglCanvasCount: 1,
    webglContextCount: 1,
    webglContextLostCount: 0,
    webglContextRestoredCount: 0,
    webglContextTypes: ["webgl2"],
    workerErrors: [],
    workerEvaluationErrors: [],
    workerUrls: workers.map(({ url }) => url),
    workers,
  };
}

describe("可视化运行时发布规则", () => {
  test("接受三类可执行 Worker、游戏连接和非空 mesh 响应", () => {
    expect(runtimeEvidenceErrors(validSnapshot())).toEqual([]);
  });

  test.each([
    ["mesh-worker-0", "mesh worker was not observed"],
    ["mesh-worker-urgent-0", "urgent mesh worker was not observed"],
    ["light-worker-0", "light worker was not observed"],
  ])("拒绝缺少命名 Worker %s", (name, expected) => {
    const snapshot = validSnapshot();
    snapshot.workers = snapshot.workers.filter(
      (worker) => worker.name !== name,
    );
    expect(runtimeEvidenceErrors(snapshot)).toContain(expected);
  });

  test("拒绝空 mesh、Worker 错误和不可执行 Worker", () => {
    const snapshot = validSnapshot();
    snapshot.nonEmptyMeshResponseCount = 0;
    snapshot.workers[0].evaluable = false;
    snapshot.workerErrors.push({
      kind: "error",
      message: "wasm trap",
      name: "mesh-worker-0",
      url: "mesh.js",
    });
    const errors = runtimeEvidenceErrors(snapshot);
    expect(errors).toContain("mesh worker was not evaluable");
    expect(errors).toContain(
      "no non-empty mesh geometry response was observed",
    );
    expect(errors).toContain("mesh-worker-0 error: wasm trap");
    expect(hasDeterministicRuntimeFailure(snapshot)).toBe(true);
  });

  test("拒绝缺失或丢失的 WebGL context", () => {
    const snapshot = validSnapshot();
    snapshot.webglContextCount = 0;
    snapshot.webglContextLostCount = 1;

    expect(runtimeEvidenceErrors(snapshot)).toEqual(
      expect.arrayContaining([
        "WebGL context was not observed",
        "WebGL context was lost",
      ]),
    );
  });
});
