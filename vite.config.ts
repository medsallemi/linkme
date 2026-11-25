import { defineConfig } from "vitest/config";

export default defineConfig({
  clearScreen: false,
  build: {
    rolldownOptions: { input: { main: "index.html", flight: "flight.html" } },
  },
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  test: {
    environment: "jsdom",
    restoreMocks: true,
    setupFiles: ["./src/test-setup.ts"],
  },
});
