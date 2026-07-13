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
  // 故障点只能由单个 ControlledLiveServer 实例显式选择。
  EXTRACTION_E2E_SETTLEMENT_CRASH: "",
};
const suite = selectSuite(process.argv.slice(2));

const supervisor = new ProcessSupervisor({
  cwd: REPOSITORY_ROOT,
  environment,
  logPrefix: "live-gameplay",
  timeoutMs: ORCHESTRATION_TIMEOUT_MS,
});

try {
  await supervisor.supervise(async (processes) => {
    await executeLiveVitest({
      config: suite.config,
      environment,
      label: suite.label,
      processes,
    });
  });
  console.log(`[live-gameplay] 通过，产物目录：${artifactDir}`);
} catch (error) {
  reportFailure("live-gameplay", supervisor.interruptedBy, error);
  process.exitCode = 1;
}

function selectSuite(args) {
  if (args.length === 0) {
    return { config: "vitest.live.config.ts", label: "真实十人玩法闭环" };
  }
  if (args.length === 1 && args[0] === "--crash-recovery") {
    return {
      config: "vitest.crash.config.ts",
      label: "结算进程故障与重启幂等验收",
    };
  }
  if (args.length === 1 && args[0] === "--settlement-uncertainty") {
    return {
      config: "vitest.uncertainty.config.ts",
      label: "真实 PostgreSQL COMMIT 结果不确定与宽限核对验收",
    };
  }
  throw new Error(
    "run-live-gameplay 只接受可选参数 --crash-recovery 或 --settlement-uncertainty",
  );
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
