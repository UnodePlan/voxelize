import { readFile, stat } from "node:fs/promises";
import { resolve } from "node:path";

const ENTRY_FILES = ["dist/index.js", "dist/index.mjs"];
const DECODE_ENTRY_FILES = [
  "dist/decode-message.js",
  "dist/decode-message.mjs",
];
const WORKER_REFERENCE = /assets\/mesh-worker-[A-Za-z0-9_-]+\.js/gu;
const RENDERING_WORKER_MARKER =
  /(?:cull-worker|mesh-worker|light-worker|cloud-worker|interval-worker)/u;
const WASM_DATA_URL = "data:application/wasm;base64,";
const MAX_WORKER_BYTES = 500_000;

const references = new Set();
for (const entry of ENTRY_FILES) {
  const content = await readFile(resolve(entry), "utf8");
  for (const match of content.matchAll(WORKER_REFERENCE)) {
    references.add(match[0]);
  }
}

if (references.size === 0) {
  throw new Error("Core bundle does not reference a mesh worker asset");
}

for (const reference of references) {
  const path = resolve("dist", reference);
  const [content, metadata] = await Promise.all([
    readFile(path, "utf8"),
    stat(path),
  ]);
  const dataUrlCount = content.split(WASM_DATA_URL).length - 1;
  if (dataUrlCount !== 1) {
    throw new Error(
      `${reference} must contain exactly one WASM data URL, received ${dataUrlCount}`,
    );
  }
  if (/new URL\(\s*["'`]data:application\/wasm/iu.test(content)) {
    throw new Error(`${reference} contains an unsafe static new URL(data:)`);
  }
  if (content.includes("async function __wbg_init")) {
    throw new Error(`${reference} retained the unused async WASM initializer`);
  }
  if (metadata.size >= MAX_WORKER_BYTES) {
    throw new Error(
      `${reference} is ${metadata.size} bytes; expected less than ${MAX_WORKER_BYTES}`,
    );
  }
}

for (const entry of DECODE_ENTRY_FILES) {
  const content = await readFile(resolve(entry), "utf8");
  if (RENDERING_WORKER_MARKER.test(content)) {
    throw new Error(`${entry} must not load rendering workers`);
  }
}
