import { STARTUP_TIMEOUT_MS } from "./live-smoke-config.mjs";

export async function executeLiveVitest({
  config,
  environment,
  label,
  processes,
}) {
  const commandOptions = {
    environment,
    timeoutMs: STARTUP_TIMEOUT_MS,
  };
  await processes.runCommand(
    "数据库 migration",
    "cargo",
    [
      "run",
      "--manifest-path",
      "apps/extraction-server/Cargo.toml",
      "--bin",
      "migrate",
      "--locked",
    ],
    commandOptions,
  );
  await processes.runCommand(
    label,
    "pnpm",
    [
      "--filter",
      "@voxelize/extraction-e2e",
      "exec",
      "vitest",
      "--run",
      "--config",
      config,
    ],
    commandOptions,
  );
}
