import { fileURLToPath } from "node:url";

import {
  createUniqueArtifactPath,
  DEFAULT_CLIENT_ORIGIN,
  DEFAULT_DATABASE_URL,
  DEFAULT_SERVER_ORIGIN,
  readExclusiveDatabaseUrl,
  selectSmokeGate,
} from "./live-smoke-config.mjs";
import { executeLiveSmoke } from "./live-smoke-orchestration.mjs";
import { ProcessSupervisor } from "./process-supervisor.mjs";

const REPOSITORY_ROOT = fileURLToPath(new URL("../../../", import.meta.url));
const ORCHESTRATION_TIMEOUT_MS = 15 * 60_000;
const gate = selectSmokeGate(process.argv.slice(2));
const serverOrigin = gate.serverOrigin ?? DEFAULT_SERVER_ORIGIN;
const clientOrigin = gate.clientOrigin ?? DEFAULT_CLIENT_ORIGIN;
let supervisor = null;

try {
  const databaseUrl = readExclusiveDatabaseUrl(
    process.env.DATABASE_URL ?? DEFAULT_DATABASE_URL,
  );
  const artifactDir = await createUniqueArtifactPath(REPOSITORY_ROOT);
  const commonEnvironment = { ...process.env, DATABASE_URL: databaseUrl };
  supervisor = new ProcessSupervisor({
    cwd: REPOSITORY_ROOT,
    environment: commonEnvironment,
    logPrefix: "live-e2e",
    timeoutMs: ORCHESTRATION_TIMEOUT_MS,
  });
  await supervisor.supervise((processes) =>
    executeLiveSmoke({
      artifactDir,
      clientOrigin,
      commonEnvironment,
      gate,
      processes,
      serverOrigin,
    }),
  );
  console.log(`[live-e2e] ${gate.name} 通过，产物目录：${artifactDir}`);
} catch (error) {
  reportFailure(supervisor?.interruptedBy ?? null, error);
  process.exitCode = 1;
}

function reportFailure(interruptedBy, error) {
  if (interruptedBy !== null) {
    console.error(`[live-e2e] 收到 ${interruptedBy}，已中止运行`);
  } else {
    console.error("[live-e2e] 运行失败", error);
  }
}
