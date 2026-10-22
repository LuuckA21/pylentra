import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { svelteTesting } from "@testing-library/svelte/vite";

export default defineConfig({
  plugins: [svelte(), svelteTesting()],
  // Tauri expects a fixed port (see devUrl in tauri.conf.json).
  server: { port: 5173, strictPort: true },
  clearScreen: false,
  test: { environment: "jsdom" },
});
