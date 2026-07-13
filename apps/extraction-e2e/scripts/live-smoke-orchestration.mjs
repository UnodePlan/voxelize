import { isChildRunning } from "./child-process-state.mjs";
import { bindAddress, STARTUP_TIMEOUT_MS } from "./live-smoke-config.mjs";

export async function executeLiveSmoke({
  artifactDir,
  clientOrigin,
  commonEnvironment,
  gate,
  processes,
  serverOrigin,
  waitForReady = waitForHttpReady,
}) {
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
    { environment: commonEnvironment, timeoutMs: STARTUP_TIMEOUT_MS },
  );

  if (gate.startServer) {
    const server = processes.startCommand(
      "Engine 服务",
      "cargo",
      [
        "run",
        "--manifest-path",
        "apps/extraction-server/Cargo.toml",
        "--bin",
        "voxelize-extraction-server",
        "--features",
        "engine",
        "--locked",
      ],
      {
        environment: {
          ...commonEnvironment,
          EXTRACTION_AUTH_LOGIN_ENABLED: "true",
          EXTRACTION_COOKIE_SECURE: "false",
          EXTRACTION_MATCHMAKING_ENABLED: "true",
          EXTRACTION_PUBLIC_ORIGIN: clientOrigin,
          EXTRACTION_SERVER_BIND: bindAddress(serverOrigin),
          EXTRACTION_SIWE_DOMAIN: new URL(clientOrigin).host,
          EXTRACTION_SIWE_URI: clientOrigin,
        },
      },
    );
    await waitForReady(
      `${serverOrigin}/health/ready`,
      { Origin: clientOrigin },
      server,
      "Engine readiness",
    );
  }

  const client = processes.startCommand(
    "live-e2e Vite",
    "pnpm",
    [
      "--filter",
      "@voxelize/extraction-client",
      "exec",
      "vite",
      "--mode",
      "live-e2e",
      "--host",
      "127.0.0.1",
      "--port",
      new URL(clientOrigin).port,
      "--strictPort",
    ],
    {
      environment: {
        ...commonEnvironment,
        EXTRACTION_E2E_PUBLIC_ORIGIN: clientOrigin,
        EXTRACTION_E2E_SERVER_TARGET: serverOrigin,
        VITE_EXTRACTION_API_URL: "",
      },
    },
  );
  await waitForReady(clientOrigin, {}, client, "Vite readiness");

  await processes.runCommand(
    gate.label,
    "pnpm",
    [
      "--filter",
      "@voxelize/extraction-e2e",
      "exec",
      "playwright",
      "test",
      gate.spec,
      "--config",
      "playwright.config.ts",
      "--grep",
      gate.grep,
    ],
    {
      environment: {
        ...commonEnvironment,
        EXTRACTION_E2E_ARTIFACT_DIR: artifactDir,
        EXTRACTION_E2E_CLIENT_URL: clientOrigin,
        EXTRACTION_E2E_HEADED: process.env.EXTRACTION_E2E_HEADED ?? "true",
        EXTRACTION_E2E_PUBLIC_ORIGIN: clientOrigin,
        EXTRACTION_E2E_SERVER_URL: serverOrigin,
      },
      timeoutMs: STARTUP_TIMEOUT_MS,
    },
  );
}

export async function waitForHttpReady(url, headers, child, label) {
  const deadline = Date.now() + STARTUP_TIMEOUT_MS;
  let lastResult = "尚未响应";
  while (Date.now() < deadline) {
    if (!isChildRunning(child)) throw new Error(`${label}前对应进程已退出`);
    try {
      const response = await fetch(url, {
        headers,
        signal: AbortSignal.timeout(2_000),
      });
      lastResult = `HTTP ${response.status}`;
      await response.arrayBuffer();
      if (response.ok) {
        console.log(`[live-e2e] ${label} 已就绪`);
        return;
      }
    } catch (error) {
      lastResult = error instanceof Error ? error.message : String(error);
    }
    await delay(250);
  }
  throw new Error(`${label}超时：${lastResult}`);
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}
