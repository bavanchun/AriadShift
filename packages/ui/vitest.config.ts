import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  test: {
    name: "ui",
    environment: "jsdom",
    globals: true,
    include: ["test/**/*.test.tsx"],
    setupFiles: ["test/setup.ts"],
  },
});
