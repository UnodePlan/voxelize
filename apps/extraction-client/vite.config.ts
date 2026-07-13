import { fileURLToPath } from "node:url";

import { defineConfig } from "vite";

const testCoreFacade = fileURLToPath(
  new URL("./src/testing/core-facade.ts", import.meta.url),
);

export default defineConfig({
  resolve: {
    alias:
      process.env.VITEST === "true"
        ? { "@voxelize/core": testCoreFacade }
        : undefined,
  },
  server: {
    host: "127.0.0.1",
    port: 5173,
    proxy: {
      "/api": { target: "http://127.0.0.1:4100" },
      "/health": { target: "http://127.0.0.1:4100" },
      "/ws": { target: "ws://127.0.0.1:4100", ws: true },
    },
  },
  preview: {
    host: "127.0.0.1",
    port: 4173,
  },
});
