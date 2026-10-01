import { defineConfig } from "vitest/config";
import vue from "@vitejs/plugin-vue";
export default defineConfig({
  plugins: [vue()],
  test: {
    environment: "jsdom",
    setupFiles: ["tests/setup.ts"],
    clearMocks: true,
    coverage: {
      provider: "v8",
      include: ["src/**/*.{ts,vue}"],
      reporter: ["text", "json", "json-summary", "lcov"],
      thresholds: { lines: 95, statements: 80, functions: 80, branches: 80 },
    },
  },
});
