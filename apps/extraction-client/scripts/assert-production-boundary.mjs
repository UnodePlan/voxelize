import { readdir, readFile, stat } from "node:fs/promises";
import { resolve } from "node:path";

const forbidden = [
  "__VOXEL_EXTRACTION_E2E__",
  "__VOXEL_EXTRACTION_LIVE_E2E__",
  "e2e-main",
  "live-e2e-main",
  "core-facade",
  "single-player-mode",
  "single-extraction-beacon",
  // DEV-only entry modules must not ship in production chunks
  "dev-multi-main",
  "startDevMultiplayerClient",
  "startSinglePlayerClient",
];
const forbiddenPatterns = [
  /\/assets\/data:application\/wasm/iu,
  /\/@fs\/[^"'`\s]*data:application\/wasm/iu,
  /new URL\(\s*["'`]data:application\/wasm/iu,
];
const inlineWorkerSource = /\bencodedJs=(["'])([A-Za-z0-9+/=]+)\1/gu;
const renderingWorkerMarker =
  /(?:cull-worker|mesh-worker|light-worker|cloud-worker|interval-worker)/u;
const files = await collectFiles(resolve("dist"));
let protocolWorkerCount = 0;

for (const file of files) {
  if (!/\.(?:html|js|css)$/u.test(file)) continue;
  const content = await readFile(file, "utf8");
  const token = forbidden.find((candidate) => content.includes(candidate));
  if (token !== undefined) {
    throw new Error(`production bundle contains test bridge token ${token}`);
  }
  const pattern = forbiddenPatterns.find((candidate) =>
    candidate.test(content),
  );
  if (pattern !== undefined) {
    throw new Error(`production bundle contains unsafe WASM URL in ${file}`);
  }
  for (const match of content.matchAll(inlineWorkerSource)) {
    const worker = Buffer.from(match[2], "base64").toString("utf8");
    if (!isProtocolWorker(worker)) continue;
    protocolWorkerCount += 1;
    if (renderingWorkerMarker.test(worker)) {
      throw new Error("protocol decoder bundle contains rendering workers");
    }
  }
}

if (protocolWorkerCount !== 1) {
  throw new Error(
    `production bundle must contain one protocol decoder worker, received ${protocolWorkerCount}`,
  );
}

async function collectFiles(directory) {
  const entries = await readdir(directory);
  const nested = await Promise.all(
    entries.map(async (entry) => {
      const path = resolve(directory, entry);
      return (await stat(path)).isDirectory() ? collectFiles(path) : [path];
    }),
  );
  return nested.flat();
}

function isProtocolWorker(source) {
  return (
    source.includes("generation") &&
    source.includes("sequence") &&
    source.includes("postMessage")
  );
}
