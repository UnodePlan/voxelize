import { defineConfig } from "vite";

export default defineConfig({
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
