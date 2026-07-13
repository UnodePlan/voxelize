import { randomUUID } from "node:crypto";
import { access } from "node:fs/promises";
import { resolve } from "node:path";

export const DEFAULT_SERVER_ORIGIN = "http://127.0.0.1:4211";
export const DEFAULT_CLIENT_ORIGIN = "http://127.0.0.1:5211";
export const STARTUP_TIMEOUT_MS = 600_000;
const DATABASE_NAME = "voxelize_extraction_e2e";
export const DEFAULT_DATABASE_URL = `postgres://localhost/${DATABASE_NAME}`;

export function selectSmokeGate(args) {
  if (args.length === 0) {
    return {
      grep: "@smoke",
      label: "Playwright 真实容量 smoke",
      name: "smoke",
      spec: "src/live/capacity.live.spec.ts",
      startServer: true,
    };
  }
  if (args.length === 1 && args[0] === "--10p") {
    return {
      grep: "@10p",
      label: "Playwright 真实十浏览器发布门禁",
      name: "十浏览器门禁",
      spec: "src/live/capacity-10p.live.spec.ts",
      startServer: true,
    };
  }
  if (args.length === 1 && args[0] === "--multi-round") {
    return {
      clientOrigin: "http://127.0.0.1:5215",
      grep: "@multi-round",
      label: "Playwright 同页连续三局资源门禁",
      name: "同页连续三局门禁",
      serverOrigin: "http://127.0.0.1:4215",
      spec: "src/live/multi-round-browser.live.spec.ts",
      startServer: false,
    };
  }
  throw new Error("run-live-smoke 只接受可选参数 --10p 或 --multi-round");
}

export function readExclusiveDatabaseUrl(value) {
  const url = new URL(value);
  const localHosts = new Set(["localhost", "127.0.0.1", "[::1]"]);
  // migration 只能触达约定的本机专用库，避免误改共享或生产数据库。
  if (
    !["postgres:", "postgresql:"].includes(url.protocol) ||
    !localHosts.has(url.hostname) ||
    url.pathname !== `/${DATABASE_NAME}` ||
    url.hash !== ""
  ) {
    throw new Error(`DATABASE_URL 必须指向本机独占数据库 ${DATABASE_NAME}`);
  }
  return url.toString();
}

export function bindAddress(origin) {
  const url = new URL(origin);
  if (url.port === "") throw new Error("server origin 必须显式指定端口");
  return `${url.hostname}:${url.port}`;
}

export async function createUniqueArtifactPath(repositoryRoot) {
  for (;;) {
    const candidate = resolve(
      repositoryRoot,
      "apps/extraction-e2e/test-results",
      `live-${Date.now()}-${process.pid}-${randomUUID()}`,
    );
    try {
      await access(candidate);
    } catch (error) {
      if (error?.code === "ENOENT") return candidate;
      throw error;
    }
  }
}
