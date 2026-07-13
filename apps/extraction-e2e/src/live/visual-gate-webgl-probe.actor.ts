import type { Page } from "@playwright/test";
import { afterEach, describe, expect, test, vi } from "vitest";

import { installWebGlProbe, readWebGlProbe } from "./visual-gate-webgl-probe";

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("WebGL 生命周期探针", () => {
  test("保留 getContext 语义并只统计唯一 canvas/context", async () => {
    const testWindow = {};
    vi.stubGlobal("window", testWindow);
    vi.stubGlobal("HTMLCanvasElement", FakeCanvas);
    const page = executingPage();

    await installWebGlProbe(page);
    const canvas = new FakeCanvas();
    const first = canvas.getContext("webgl2");
    const second = canvas.getContext("webgl2");
    canvas.getContext("2d");
    canvas.dispatchEvent(new Event("webglcontextlost"));
    canvas.dispatchEvent(new Event("webglcontextrestored"));

    expect(first).toBe(canvas.webglContext);
    expect(second).toBe(first);
    expect(canvas.calls).toEqual(["webgl2", "webgl2", "2d"]);
    expect(await readWebGlProbe(page)).toEqual({
      canvasCount: 1,
      contextCount: 1,
      contextLostCount: 1,
      contextRestoredCount: 1,
      contextTypes: ["webgl2"],
    });
  });
});

class FakeCanvas extends EventTarget {
  readonly calls: string[] = [];
  readonly webglContext = {};

  getContext(type: string): object | null {
    this.calls.push(type);
    if (type === "webgl" || type === "webgl2") return this.webglContext;
    return type === "2d" ? {} : null;
  }
}

function executingPage(): Page {
  return {
    async addInitScript(script: unknown, argument: unknown) {
      (script as (value: unknown) => void)(argument);
    },
    async evaluate(script: unknown, argument: unknown) {
      return (script as (value: unknown) => unknown)(argument);
    },
  } as unknown as Page;
}
