import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    hookTimeout: 1_200_000,
    include: ["src/live/**/*.live.ts"],
    testTimeout: 900_000,
  },
});
