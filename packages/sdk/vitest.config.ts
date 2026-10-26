import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    name: "sdk",
    environment: "node",
    include: ["test/**/*.test.ts"],
    // The client is generated, not committed: write it before any test imports it.
    globalSetup: ["test/generate-client.ts"],
  },
});
