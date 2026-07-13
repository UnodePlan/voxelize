import type { Page } from "@playwright/test";

const WORKER_PROBE_KEY = "__VOXEL_EXTRACTION_VISUAL_WORKER_PROBE__";
const MAX_PROBE_ENTRIES = 50;

export interface WorkerObservation {
  name: string;
  url: string;
}

export interface WorkerProbeError extends WorkerObservation {
  kind: "error" | "messageerror";
  message: string;
}

export interface VisualWorkerProbeSnapshot {
  errors: WorkerProbeError[];
  meshResponseCount: number;
  nonEmptyMeshResponseCount: number;
  observed: WorkerObservation[];
}

/** 必须在首次导航前安装，才能观察 Vite 在产品入口创建的全部 Worker。 */
export async function installVisualWorkerProbe(page: Page): Promise<void> {
  await page.addInitScript(
    ({ key, maxEntries }) => {
      type MutableProbe = VisualWorkerProbeSnapshot;

      if (Reflect.get(window, key) !== undefined) return;

      const state: MutableProbe = {
        errors: [],
        meshResponseCount: 0,
        nonEmptyMeshResponseCount: 0,
        observed: [],
      };
      Object.defineProperty(window, key, {
        configurable: false,
        enumerable: false,
        value: state,
        writable: false,
      });

      const NativeWorker = window.Worker;
      if (typeof NativeWorker !== "function") return;

      const limitText = (value: string, maxLength: number): string =>
        value.length <= maxLength
          ? value
          : `${value.slice(0, maxLength)}...(${value.length} chars)`;
      const pushBounded = <T>(target: T[], value: T): void => {
        if (target.length < maxEntries) target.push(value);
      };
      const isMeshWorker = (name: string): boolean =>
        /^mesh-worker(?:-urgent)?-\d+$/u.test(name);

      const observeWorker = (
        worker: Worker,
        scriptURL: unknown,
        options: unknown,
      ): void => {
        let workerName = "";
        let normalizedScriptURL = "unreadable worker URL";
        try {
          normalizedScriptURL = String(scriptURL);
        } catch {
          // 观察失败不能改变原生 Worker 已成功完成的构造语义。
        }
        try {
          if (options !== null && typeof options === "object") {
            const rawName = Reflect.get(options, "name");
            workerName = rawName === undefined ? "" : String(rawName);
          }
        } catch {
          workerName = "unreadable worker name";
        }
        const observation = {
          name: limitText(workerName, 160),
          url: limitText(normalizedScriptURL, 512),
        };
        pushBounded(state.observed, observation);

        const recordError = (
          kind: WorkerProbeError["kind"],
          event: Event,
        ): void => {
          const rawMessage = Reflect.get(event, "message");
          const fallback =
            kind === "error"
              ? "worker execution failed"
              : "worker message could not be deserialized";
          pushBounded(state.errors, {
            ...observation,
            kind,
            message: limitText(
              typeof rawMessage === "string" && rawMessage.length > 0
                ? rawMessage
                : fallback,
              512,
            ),
          });
        };

        worker.addEventListener("error", (event) => {
          recordError("error", event);
        });
        worker.addEventListener("messageerror", (event) => {
          recordError("messageerror", event);
        });
        if (isMeshWorker(observation.name)) {
          worker.addEventListener("message", (event) => {
            const data = event.data as { geometries?: unknown } | null;
            const geometries =
              data !== null && typeof data === "object"
                ? data.geometries
                : undefined;
            if (!Array.isArray(geometries)) return;
            state.meshResponseCount += 1;
            if (geometries.some(hasRenderableTriangle)) {
              state.nonEmptyMeshResponseCount += 1;
            }
          });
        }
      };
      const hasRenderableTriangle = (value: unknown): boolean => {
        if (value === null || typeof value !== "object") return false;
        const positions = Reflect.get(value, "positions");
        const indices = Reflect.get(value, "indices");
        return (
          positions instanceof Float32Array &&
          positions.length >= 9 &&
          (indices instanceof Uint16Array || indices instanceof Uint32Array) &&
          indices.length >= 3
        );
      };
      const ObservedWorker = new Proxy(NativeWorker, {
        construct(target, argumentsList, newTarget) {
          const worker = Reflect.construct(
            target,
            argumentsList,
            newTarget,
          ) as Worker;
          observeWorker(worker, argumentsList[0], argumentsList[1]);
          return worker;
        },
      });

      Object.defineProperty(window, "Worker", {
        configurable: true,
        value: ObservedWorker,
        writable: true,
      });
    },
    { key: WORKER_PROBE_KEY, maxEntries: MAX_PROBE_ENTRIES },
  );
}

export async function readVisualWorkerProbe(
  page: Page,
): Promise<VisualWorkerProbeSnapshot> {
  return page.evaluate(
    ({ key, maxEntries }) => {
      const probe = Reflect.get(window, key) as
        | VisualWorkerProbeSnapshot
        | undefined;
      if (probe === undefined) {
        return {
          errors: [],
          meshResponseCount: 0,
          nonEmptyMeshResponseCount: 0,
          observed: [],
        };
      }
      return {
        errors: probe.errors.slice(0, maxEntries),
        meshResponseCount: probe.meshResponseCount,
        nonEmptyMeshResponseCount: probe.nonEmptyMeshResponseCount,
        observed: probe.observed.slice(0, maxEntries),
      };
    },
    { key: WORKER_PROBE_KEY, maxEntries: MAX_PROBE_ENTRIES },
  );
}
