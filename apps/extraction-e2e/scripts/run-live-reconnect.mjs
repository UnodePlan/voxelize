import { fileURLToPath } from "node:url";

import { createUniqueArtifactPath } from "./live-smoke-config.mjs";
import { executeLiveVitest } from "./live-vitest-orchestration.mjs";
import { ProcessSupervisor } from "./process-supervisor.mjs";

const REPOSITORY_ROOT = fileURLToPath(new URL("../../../", import.meta.url));
const DATABASE_NAME = "voxelize_extraction_e2e";
const DEFAULT_DATABASE_URL = `postgres://localhost/${DATABASE_NAME}`;
const ORCHESTRATION_TIMEOUT_MS = 30 * 60_000;
const databaseUrl = readExclusiveDatabaseUrl(
  process.env.DATABASE_URL ?? DEFAULT_DATABASE_URL,
);
const artifactDir = await createUniqueArtifactPath(REPOSITORY_ROOT);
const environment = {
  ...process.env,
  DATABASE_URL: databaseUrl,
  EXTRACTION_E2E_ARTIFACT_DIR: artifactDir,
  // 重连门禁绝不继承结算进程退出注入。
  EXTRACTION_E2E_SETTLEMENT_CRASH: "",
};

if (process.argv.length !== 2) {
  throw new Error("run-live-reconnect 不接受命令行参数");
}

const supervisor = new ProcessSupervisor({
  cwd: REPOSITORY_ROOT,
  environment,
  logPrefix: "live-reconnect",
  timeoutMs: ORCHESTRATION_TIMEOUT_MS,
});

try {
  await supervisor.supervise(async (processes) => {
    await executeLiveVitest({
      config: "vitest.reconnect.config.ts",
      environment,
      label: "断线重连真实门禁",
      processes,
    });
  });
  console.log(`[live-reconnect] 通过，产物目录：${artifactDir}`);
} catch (error) {
  reportFailure("live-reconnect", supervisor.interruptedBy, error);
  process.exitCode = 1;
}

function readExclusiveDatabaseUrl(value) {
  const url = new URL(value);
  if (
    !["postgres:", "postgresql:"].includes(url.protocol) ||
    !["localhost", "127.0.0.1", "[::1]"].includes(url.hostname) ||
    url.pathname !== `/${DATABASE_NAME}` ||
    url.hash !== ""
  ) {
    throw new Error(`DATABASE_URL 必须指向本机独占数据库 ${DATABASE_NAME}`);
  }
  return url.toString();
}

function reportFailure(prefix, interruptedBy, error) {
  if (interruptedBy !== null) {
    console.error(`[${prefix}] 收到 ${interruptedBy}，已中止运行`);
  } else {
    console.error(`[${prefix}] 运行失败`, error);
  }
}
