const DEFAULT_SERVER_URL = "http://127.0.0.1:4211";
const DEFAULT_CLIENT_URL = "http://127.0.0.1:5211";
const DEFAULT_SCENARIO_TIMEOUT_MS = 120_000;
const DEFAULT_POLL_INTERVAL_MS = 100;
const DEFAULT_CONNECTION_GRACE_MS = 3_000;

type Environment = Readonly<Record<string, string | undefined>>;

export interface LiveE2eConfig {
  artifactDir: string;
  clientUrl: string;
  connectionGraceMs: number;
  pollIntervalMs: number;
  protocolWebSocketUrl: string;
  publicOrigin: string;
  scenarioTimeoutMs: number;
  serverUrl: string;
}

export function loadLiveE2eConfig(
  environment: Environment = process.env,
): LiveE2eConfig {
  const serverUrl = readHttpOrigin(
    environment.EXTRACTION_E2E_SERVER_URL ?? DEFAULT_SERVER_URL,
    "EXTRACTION_E2E_SERVER_URL",
  );
  const clientUrl = readHttpOrigin(
    environment.EXTRACTION_E2E_CLIENT_URL ?? DEFAULT_CLIENT_URL,
    "EXTRACTION_E2E_CLIENT_URL",
  );
  const publicOrigin = readHttpOrigin(
    environment.EXTRACTION_E2E_PUBLIC_ORIGIN ?? clientUrl,
    "EXTRACTION_E2E_PUBLIC_ORIGIN",
  );
  if (publicOrigin !== clientUrl) {
    throw new Error(
      "EXTRACTION_E2E_PUBLIC_ORIGIN must equal the live client origin",
    );
  }

  return {
    artifactDir:
      readOptionalNonBlank(
        environment.EXTRACTION_E2E_ARTIFACT_DIR,
        "EXTRACTION_E2E_ARTIFACT_DIR",
      ) ?? `test-results/live-${Date.now()}-${process.pid}`,
    clientUrl,
    connectionGraceMs: readPositiveInteger(
      environment.EXTRACTION_E2E_CONNECTION_GRACE_MS,
      DEFAULT_CONNECTION_GRACE_MS,
      "EXTRACTION_E2E_CONNECTION_GRACE_MS",
    ),
    pollIntervalMs: readPositiveInteger(
      environment.EXTRACTION_E2E_POLL_INTERVAL_MS,
      DEFAULT_POLL_INTERVAL_MS,
      "EXTRACTION_E2E_POLL_INTERVAL_MS",
    ),
    protocolWebSocketUrl: websocketUrl(serverUrl),
    publicOrigin,
    scenarioTimeoutMs: readPositiveInteger(
      environment.EXTRACTION_E2E_TIMEOUT_MS,
      DEFAULT_SCENARIO_TIMEOUT_MS,
      "EXTRACTION_E2E_TIMEOUT_MS",
    ),
    serverUrl,
  };
}

function readOptionalNonBlank(
  value: string | undefined,
  name: string,
): string | null {
  if (value === undefined) return null;
  if (value.trim() === "")
    throw new Error(`${name}: expected a non-blank path`);
  return value;
}

function readHttpOrigin(value: string, name: string): string {
  const url = new URL(value);
  if (
    (url.protocol !== "http:" && url.protocol !== "https:") ||
    url.username !== "" ||
    url.password !== "" ||
    url.pathname !== "/" ||
    url.search !== "" ||
    url.hash !== ""
  ) {
    throw new Error(`${name}: expected an HTTP(S) origin without credentials`);
  }
  return url.origin;
}

function readPositiveInteger(
  value: string | undefined,
  fallback: number,
  name: string,
): number {
  if (value === undefined) return fallback;
  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed <= 0) {
    throw new Error(`${name}: expected a positive safe integer`);
  }
  return parsed;
}

function websocketUrl(serverUrl: string): string {
  const url = new URL(serverUrl);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.pathname = "/ws/";
  return url.toString();
}
