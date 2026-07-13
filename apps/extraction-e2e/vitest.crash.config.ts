import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    hookTimeout: 1_200_000,
    include: ["src/live/crash-recovery.crash.ts"],
    testTimeout: 900_000,
  },
});
