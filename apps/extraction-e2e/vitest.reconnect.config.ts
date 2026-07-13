import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    fileParallelism: false,
    hookTimeout: 1_200_000,
    include: ["src/live/reconnect.reconnect.ts"],
    sequence: { concurrent: false },
    testTimeout: 900_000,
  },
});
