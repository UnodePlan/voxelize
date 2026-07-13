import type {
  Browser,
  Page,
  Worker as PlaywrightWorker,
} from "@playwright/test";
import type { Wallet } from "ethers";
import { afterEach, describe, expect, it, vi } from "vitest";

import { BrowserCapacityActor } from "./browser-actor";
import type { LiveE2eConfig } from "./config";
import type { PrimaryAdmissionBarrier } from "./queue";
import { LiveRuntimeDiagnostics } from "./visual-gate-diagnostics";
import {
  installVisualWorkerProbe,
  readVisualWorkerProbe,
} from "./visual-gate-worker-probe";

vi.mock("./siwe", () => ({
  authenticateEoa: vi.fn(async () => undefined),
}));

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("visual gate worker probe", () => {
  it("records named mesh responses and bounded worker failures", async () => {
    const testWindow = { Worker: FakeWorker };
    vi.stubGlobal("window", testWindow);
    const page = initScriptPage();

    await installVisualWorkerProbe(page);
    const meshWorker = new testWindow.Worker("mesh.js", {
      name: "mesh-worker-0",
    });
    expect(meshWorker).toBeInstanceOf(FakeWorker);
    expect(meshWorker).toBeInstanceOf(testWindow.Worker);
    let nameReads = 0;
    let urlConversions = 0;
    const workerUrl = {
      toString() {
        urlConversions += 1;
        return "urgent-mesh.js";
      },
    } as unknown as URL;
    const urgentWorker = new testWindow.Worker(workerUrl, {
      get name() {
        nameReads += 1;
        return "mesh-worker-urgent-0";
      },
    });
    expect(urgentWorker.scriptURL).toBe(workerUrl);
    expect(nameReads).toBe(2);
    expect(urlConversions).toBe(2);
    const nullOptionsWorker = new (testWindow.Worker as unknown as new (
      scriptURL: string,
      options: null,
    ) => FakeWorker)("null-options.js", null);
    expect(nullOptionsWorker.options).toBeNull();
    meshWorker.dispatchEvent(messageEvent({ geometries: [] }));
    meshWorker.dispatchEvent(messageEvent({ geometries: [{}] }));
    meshWorker.dispatchEvent(
      messageEvent({
        geometries: [
          {
            indices: new Uint16Array([0, 1, 2]),
            positions: new Float32Array(9),
          },
        ],
      }),
    );
    meshWorker.dispatchEvent(workerEvent("error", "WASM init failed"));
    meshWorker.dispatchEvent(workerEvent("messageerror"));
    const lightWorker = new testWindow.Worker("light.js", {
      name: "light-worker-0",
    });
    lightWorker.dispatchEvent(messageEvent({ geometries: [{}] }));
    for (let index = 0; index < 55; index += 1) {
      new testWindow.Worker(`worker-${index}.js`, {
        name: `light-worker-${index}`,
      }).dispatchEvent(workerEvent("error", `failure-${index}`));
    }

    const snapshot = await readVisualWorkerProbe(page);
    expect(snapshot.observed).toHaveLength(50);
    expect(snapshot.errors).toHaveLength(50);
    expect(snapshot.observed[0]).toEqual({
      name: "mesh-worker-0",
      url: "mesh.js",
    });
    expect(snapshot.errors.slice(0, 2)).toEqual([
      {
        kind: "error",
        message: "WASM init failed",
        name: "mesh-worker-0",
        url: "mesh.js",
      },
      {
        kind: "messageerror",
        message: "worker message could not be deserialized",
        name: "mesh-worker-0",
        url: "mesh.js",
      },
    ]);
    expect(snapshot.meshResponseCount).toBe(3);
    expect(snapshot.nonEmptyMeshResponseCount).toBe(1);
  });

  it("installs before diagnostics are created and merges live worker names", async () => {
    const operations: string[] = [];
    const page = diagnosticsPage(operations);

    const diagnostics = await LiveRuntimeDiagnostics.create(
      page,
      "http://127.0.0.1:5173",
    );
    operations.push("goto");
    const snapshot = await diagnostics.snapshot();

    const gotoIndex = operations.indexOf("goto");
    expect(gotoIndex).toBeGreaterThan(0);
    expect(operations.slice(0, gotoIndex)).toEqual(
      Array.from({ length: gotoIndex }, () => "addInitScript"),
    );
    expect(snapshot.workers).toEqual([
      {
        evaluable: true,
        name: "mesh-worker-0",
        url: "http://127.0.0.1:5173/mesh.js",
      },
      {
        evaluable: false,
        name: "",
        url: "",
      },
    ]);
    expect(snapshot.workerEvaluationErrors).toEqual([
      "inline worker: worker stopped",
    ]);
    expect(snapshot.workerErrors).toEqual([]);
  });

  it("bounds a stalled runtime diagnostics snapshot", async () => {
    const page = diagnosticsPage([]);
    Object.assign(page, {
      workers: () => [
        {
          evaluate: () => new Promise(() => undefined),
          url: () => "stalled-worker.js",
        },
      ],
    });
    const diagnostics = await LiveRuntimeDiagnostics.create(
      page,
      "http://127.0.0.1:5173",
    );

    await expect(diagnostics.snapshot(10)).rejects.toThrow(
      "runtime diagnostics snapshot exceeded 10ms",
    );
  });

  it("installs the probe before BrowserCapacityActor first navigates", async () => {
    const operations: string[] = [];
    const page = actorPage(operations);
    const context = {
      async addInitScript() {
        operations.push("context.addInitScript");
      },
      async close() {
        operations.push("context.close");
      },
      async newPage() {
        return page;
      },
      request: {},
    };
    const browser = {
      async newContext() {
        return context;
      },
    } as unknown as Browser;
    const actor = new BrowserCapacityActor(
      "browser-0",
      browser,
      { address: "0x0000000000000000000000000000000000000001" } as Wallet,
      {} as PrimaryAdmissionBarrier,
      liveConfig(),
      { label: "desktop", viewport: { height: 900, width: 1440 } },
    );

    await actor.connect();
    expect(operations.indexOf("page.addInitScript")).toBeLessThan(
      operations.indexOf("page.goto"),
    );
    await actor.disconnect();
  });
});

class FakeWorker extends EventTarget {
  constructor(
    readonly scriptURL: string | URL,
    readonly options?: WorkerOptions,
  ) {
    super();
    String(scriptURL);
    options?.credentials;
    options?.name;
    options?.type;
  }
}

function initScriptPage(): Page {
  return {
    async addInitScript(script: unknown, argument: unknown) {
      (script as (value: unknown) => void)(argument);
    },
    async evaluate(script: unknown, argument: unknown) {
      return (script as (value: unknown) => unknown)(argument);
    },
  } as unknown as Page;
}

function diagnosticsPage(operations: string[]): Page {
  const workers = [
    runtimeWorker("http://127.0.0.1:5173/mesh.js", {
      evaluable: true,
      name: "mesh-worker-0",
    }),
    runtimeWorker("", new Error("worker stopped")),
  ];
  return {
    async addInitScript() {
      operations.push("addInitScript");
    },
    async evaluate() {
      return {
        canvasCount: 1,
        contextCount: 1,
        contextLostCount: 0,
        contextRestoredCount: 0,
        contextTypes: ["webgl2"],
        errors: [],
        meshResponseCount: 0,
        nonEmptyMeshResponseCount: 0,
        observed: [],
      };
    },
    on() {},
    workers() {
      return workers;
    },
  } as unknown as Page;
}

function actorPage(operations: string[]): Page {
  return {
    async addInitScript() {
      operations.push("page.addInitScript");
    },
    async goto() {
      operations.push("page.goto");
    },
    locator() {
      return {
        async waitFor() {
          operations.push("lobby.waitFor");
        },
      };
    },
    on() {},
  } as unknown as Page;
}

function liveConfig(): LiveE2eConfig {
  return {
    artifactDir: "test-results",
    clientUrl: "http://127.0.0.1:5173",
    connectionGraceMs: 3_000,
    pollIntervalMs: 100,
    protocolWebSocketUrl: "ws://127.0.0.1:4211/ws/",
    publicOrigin: "http://127.0.0.1:5173",
    scenarioTimeoutMs: 1_000,
    serverUrl: "http://127.0.0.1:4211",
  };
}

function runtimeWorker(
  url: string,
  result: { evaluable: boolean; name: string } | Error,
): PlaywrightWorker {
  return {
    async evaluate() {
      if (result instanceof Error) throw result;
      return result;
    },
    url() {
      return url;
    },
  } as unknown as PlaywrightWorker;
}

function messageEvent(data: unknown): Event {
  return Object.assign(new Event("message"), { data });
}

function workerEvent(type: "error" | "messageerror", message?: string): Event {
  return Object.assign(new Event(type), { message });
}
