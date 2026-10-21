import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],
  // Tauri expects a fixed port (see devUrl in tauri.conf.json).
  server: { port: 5173, strictPort: true },
  clearScreen: false,
  test: { environment: "jsdom" },
});
