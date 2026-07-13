import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    hookTimeout: 1_200_000,
    include: ["src/live/settlement-uncertainty.uncertainty.ts"],
    testTimeout: 900_000,
  },
});
