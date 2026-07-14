import assert from "node:assert/strict";
import test from "node:test";

import {
  bindAddress,
  readExclusiveDatabaseUrl,
  readRequiredProjectId,
  selectSmokeGate,
  STARTUP_TIMEOUT_MS,
} from "./live-smoke-config.mjs";
import { executeLiveSmoke } from "./live-smoke-orchestration.mjs";
import { executeLiveVitest } from "./live-vitest-orchestration.mjs";

test("Smoke gate 保留默认、十浏览器与 multi-round 端口语义", () => {
  assert.deepEqual(selectSmokeGate([]), {
    grep: "@smoke",
    label: "Playwright 真实容量 smoke",
    name: "smoke",
    spec: "src/live/capacity.live.spec.ts",
    startServer: true,
  });
  assert.deepEqual(selectSmokeGate(["--10p"]), {
    grep: "@10p",
    label: "Playwright 真实十浏览器发布门禁",
    name: "十浏览器门禁",
    spec: "src/live/capacity-10p.live.spec.ts",
    startServer: true,
  });
  assert.deepEqual(selectSmokeGate(["--multi-round"]), {
    clientOrigin: "http://127.0.0.1:5215",
    grep: "@multi-round",
    label: "Playwright 同页连续三局资源门禁",
    name: "同页连续三局门禁",
    serverOrigin: "http://127.0.0.1:4215",
    spec: "src/live/multi-round-browser.live.spec.ts",
    startServer: false,
  });
  assert.deepEqual(selectSmokeGate(["--appkit"]), {
    clientMode: "appkit-e2e",
    clientOrigin: "http://127.0.0.1:5217",
    grep: "@appkit",
    label: "Playwright 真实 AppKit WalletKit SIWE 门禁",
    name: "AppKit WalletKit SIWE 门禁",
    requiresProjectId: true,
    serverOrigin: "http://127.0.0.1:4217",
    spec: "src/live/appkit-siwe.live.spec.ts",
    startServer: true,
  });
  assert.throws(() => selectSmokeGate(["--unknown"]), /只接受/u);
});

test("Smoke migration 只接受本机约定独占数据库", () => {
  assert.equal(
    readExclusiveDatabaseUrl("postgres://localhost/voxelize_extraction_e2e"),
    "postgres://localhost/voxelize_extraction_e2e",
  );
  assert.throws(
    () =>
      readExclusiveDatabaseUrl(
        "postgres://db.example.com/voxelize_extraction_e2e",
      ),
    /本机独占数据库/u,
  );
  assert.throws(
    () => readExclusiveDatabaseUrl("postgres://localhost/postgres"),
    /本机独占数据库/u,
  );
  assert.equal(bindAddress("http://127.0.0.1:4211"), "127.0.0.1:4211");
  assert.throws(() => bindAddress("http://127.0.0.1"), /显式指定端口/u);
  assert.equal(readRequiredProjectId(" project-id "), "project-id");
  assert.throws(() => readRequiredProjectId("  "), /VITE_REOWN_PROJECT_ID/u);
});

test("Smoke 编排保持 migration、Engine、Vite、Playwright 顺序与唯一产物", async () => {
  const events = [];
  const processes = fakeProcesses(events);
  const gate = selectSmokeGate([]);
  await executeLiveSmoke({
    artifactDir: "/tmp/unique-artifact",
    clientOrigin: "http://127.0.0.1:5211",
    commonEnvironment: { DATABASE_URL: "exclusive" },
    gate,
    processes,
    serverOrigin: "http://127.0.0.1:4211",
    waitForReady: async (url, headers, _child, label) => {
      events.push({ headers, label, type: "ready", url });
    },
  });

  assert.deepEqual(
    events.map(({ label, type }) => `${type}:${label}`),
    [
      "run:数据库 migration",
      "start:Engine 服务",
      "ready:Engine readiness",
      "start:live-e2e Vite",
      "ready:Vite readiness",
      "run:Playwright 真实容量 smoke",
    ],
  );
  assert.equal(events[0].options.timeoutMs, STARTUP_TIMEOUT_MS);
  assert.equal(
    events[1].options.environment.EXTRACTION_SERVER_BIND,
    "127.0.0.1:4211",
  );
  assert.equal(
    events.at(-1).options.environment.EXTRACTION_E2E_ARTIFACT_DIR,
    "/tmp/unique-artifact",
  );
  assert.equal(events.at(-1).options.timeoutMs, STARTUP_TIMEOUT_MS);
});

test("multi-round 不外启 Engine，仍先启 Vite 再运行指定 Playwright", async () => {
  const events = [];
  const gate = selectSmokeGate(["--multi-round"]);
  await executeLiveSmoke({
    artifactDir: "/tmp/multi-round-artifact",
    clientOrigin: gate.clientOrigin,
    commonEnvironment: { DATABASE_URL: "exclusive" },
    gate,
    processes: fakeProcesses(events),
    serverOrigin: gate.serverOrigin,
    waitForReady: async (_url, _headers, _child, label) => {
      events.push({ label, type: "ready" });
    },
  });

  assert.deepEqual(
    events.map(({ label, type }) => `${type}:${label}`),
    [
      "run:数据库 migration",
      "start:live-e2e Vite",
      "ready:Vite readiness",
      "run:Playwright 同页连续三局资源门禁",
    ],
  );
  assert.equal(
    events.some(({ label }) => label === "Engine 服务"),
    false,
  );
  assert.equal(
    events.at(-1).args.includes("src/live/multi-round-browser.live.spec.ts"),
    true,
  );
});

test("AppKit 门禁使用产品客户端模式并透传 Project ID", async () => {
  const events = [];
  const gate = selectSmokeGate(["--appkit"]);
  await executeLiveSmoke({
    artifactDir: "/tmp/appkit-artifact",
    clientOrigin: gate.clientOrigin,
    commonEnvironment: {
      DATABASE_URL: "exclusive",
      VITE_REOWN_PROJECT_ID: "should-not-leak",
    },
    gate,
    processes: fakeProcesses(events),
    projectId: " project-id ",
    serverOrigin: gate.serverOrigin,
    waitForReady: async (_url, _headers, _child, label) => {
      events.push({ label, type: "ready" });
    },
  });

  assert.deepEqual(
    events.map(({ label, type }) => `${type}:${label}`),
    [
      "run:数据库 migration",
      "start:Engine 服务",
      "ready:Engine readiness",
      "start:appkit-e2e Vite",
      "ready:Vite readiness",
      "run:Playwright 真实 AppKit WalletKit SIWE 门禁",
    ],
  );
  const vite = events.find(({ label }) => label === "appkit-e2e Vite");
  const server = events.find(({ label }) => label === "Engine 服务");
  const playwright = events.at(-1);
  assert.equal(
    server.options.environment.EXTRACTION_SERVER_BIND,
    "127.0.0.1:4217",
  );
  assert.equal(
    server.options.environment.EXTRACTION_SIWE_DOMAIN,
    "127.0.0.1:5217",
  );
  assert.equal(server.options.environment.VITE_REOWN_PROJECT_ID, undefined);
  assert.equal(events[0].options.environment.VITE_REOWN_PROJECT_ID, undefined);
  assert.equal(vite.args.includes("appkit-e2e"), true);
  assert.equal(vite.args.includes("5217"), true);
  assert.equal(
    vite.options.environment.EXTRACTION_E2E_SERVER_TARGET,
    "http://127.0.0.1:4217",
  );
  assert.equal(vite.options.environment.VITE_REOWN_PROJECT_ID, "project-id");
  assert.equal(
    playwright.options.environment.EXTRACTION_E2E_CLIENT_URL,
    "http://127.0.0.1:5217",
  );
  assert.equal(
    playwright.options.environment.EXTRACTION_E2E_SERVER_URL,
    "http://127.0.0.1:4217",
  );
  assert.equal(
    playwright.options.environment.VITE_REOWN_PROJECT_ID,
    "project-id",
  );
});

test("Vitest 真实门禁为 migration 和测试固定 600 秒并透传唯一产物目录", async () => {
  const events = [];
  const environment = {
    DATABASE_URL: "exclusive",
    EXTRACTION_E2E_ARTIFACT_DIR: "/tmp/unique-vitest-artifact",
  };

  await executeLiveVitest({
    config: "vitest.reconnect.config.ts",
    environment,
    label: "断线重连真实门禁",
    processes: fakeProcesses(events),
  });

  assert.deepEqual(
    events.map(({ label, type }) => `${type}:${label}`),
    ["run:数据库 migration", "run:断线重连真实门禁"],
  );
  assert.equal(
    events.every(({ options }) => options.timeoutMs === STARTUP_TIMEOUT_MS),
    true,
  );
  assert.equal(
    events.every(
      ({ options }) =>
        options.environment.EXTRACTION_E2E_ARTIFACT_DIR ===
        "/tmp/unique-vitest-artifact",
    ),
    true,
  );
});

function fakeProcesses(events) {
  return {
    async runCommand(label, command, args, options) {
      events.push({ args, command, label, options, type: "run" });
    },
    startCommand(label, command, args, options) {
      events.push({ args, command, label, options, type: "start" });
      return { exitCode: null, pid: 123, signalCode: null };
    },
  };
}
