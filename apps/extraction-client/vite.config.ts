import { fileURLToPath } from "node:url";

import { defineConfig } from "vite";

const extractionServerTarget =
  process.env.EXTRACTION_E2E_SERVER_TARGET ?? "http://127.0.0.1:4100";
const extractionPublicOrigin =
  process.env.EXTRACTION_E2E_PUBLIC_ORIGIN ?? "http://127.0.0.1:5173";
const extractionProxy = {
  headers: { Origin: extractionPublicOrigin },
  target: extractionServerTarget,
};

const testCoreFacade = fileURLToPath(
  new URL("./src/testing/core-facade.ts", import.meta.url),
);

export default defineConfig({
  resolve: {
    alias:
      process.env.VITEST === "true"
        ? [
            {
              find: "@voxelize/core/decode-message",
              replacement: testCoreFacade,
            },
            { find: "@voxelize/core", replacement: testCoreFacade },
          ]
        : undefined,
  },
  server: {
    host: "127.0.0.1",
    port: 5173,
    proxy: {
      "/api": extractionProxy,
      "/health": extractionProxy,
      "/ws": { ...extractionProxy, ws: true },
    },
  },
  preview: {
    host: "127.0.0.1",
    port: 4173,
  },
});
