import type { Page } from "@playwright/test";

const WEBGL_PROBE_KEY = "__VOXEL_EXTRACTION_WEBGL_PROBE__";
const MAX_CONTEXT_TYPES = 16;

export interface WebGlProbeSnapshot {
  canvasCount: number;
  contextCount: number;
  contextLostCount: number;
  contextRestoredCount: number;
  contextTypes: string[];
}

/** 在产品入口执行前观察 WebGL 创建次数，不持有 context 强引用影响回收。 */
export async function installWebGlProbe(page: Page): Promise<void> {
  await page.addInitScript(
    ({ key, maxContextTypes }) => {
      if (Reflect.get(window, key) !== undefined) return;

      const state: WebGlProbeSnapshot = {
        canvasCount: 0,
        contextCount: 0,
        contextLostCount: 0,
        contextRestoredCount: 0,
        contextTypes: [],
      };
      Object.defineProperty(window, key, {
        configurable: false,
        enumerable: false,
        value: state,
        writable: false,
      });

      const prototype = HTMLCanvasElement.prototype;
      const descriptor = Object.getOwnPropertyDescriptor(
        prototype,
        "getContext",
      );
      if (descriptor?.value === undefined) return;
      const nativeGetContext =
        descriptor.value as HTMLCanvasElement["getContext"];
      const contexts = new WeakSet<object>();
      const canvases = new WeakSet<HTMLCanvasElement>();
      const observedGetContext = new Proxy(nativeGetContext, {
        apply(target, thisArgument, argumentsList) {
          const context = Reflect.apply(target, thisArgument, argumentsList) as
            | object
            | null;
          const contextType = argumentsList[0];
          if (
            context === null ||
            typeof contextType !== "string" ||
            !/^webgl2?$/u.test(contextType) ||
            !(thisArgument instanceof HTMLCanvasElement)
          ) {
            return context;
          }
          if (!contexts.has(context)) {
            contexts.add(context);
            state.contextCount += 1;
            if (state.contextTypes.length < maxContextTypes) {
              state.contextTypes.push(contextType);
            }
          }
          if (!canvases.has(thisArgument)) {
            canvases.add(thisArgument);
            state.canvasCount += 1;
            thisArgument.addEventListener("webglcontextlost", () => {
              state.contextLostCount += 1;
            });
            thisArgument.addEventListener("webglcontextrestored", () => {
              state.contextRestoredCount += 1;
            });
          }
          return context;
        },
      });
      Object.defineProperty(prototype, "getContext", {
        ...descriptor,
        value: observedGetContext,
      });
    },
    { key: WEBGL_PROBE_KEY, maxContextTypes: MAX_CONTEXT_TYPES },
  );
}

export function readWebGlProbe(page: Page): Promise<WebGlProbeSnapshot> {
  return page.evaluate(
    ({ key, maxContextTypes }) => {
      const probe = Reflect.get(window, key) as WebGlProbeSnapshot | undefined;
      return probe === undefined
        ? {
            canvasCount: 0,
            contextCount: 0,
            contextLostCount: 0,
            contextRestoredCount: 0,
            contextTypes: [],
          }
        : {
            canvasCount: probe.canvasCount,
            contextCount: probe.contextCount,
            contextLostCount: probe.contextLostCount,
            contextRestoredCount: probe.contextRestoredCount,
            contextTypes: probe.contextTypes.slice(0, maxContextTypes),
          };
    },
    { key: WEBGL_PROBE_KEY, maxContextTypes: MAX_CONTEXT_TYPES },
  );
}
