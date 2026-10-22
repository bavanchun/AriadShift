import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    name: "ariad-wasm",
    environment: "node",
    include: ["test/**/*.test.ts"],
  },
});
