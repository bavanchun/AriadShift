import { createRequire } from "node:module";
import path from "node:path";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const require = createRequire(import.meta.url);
// Server components call next-intl's server API, which Next resolves through the
// `react-server` condition and the plugin's config alias. Tests render them in a plain Node
// process, so alias the same two entry points.
const nextIntlServer = path.join(
  path.dirname(require.resolve("next-intl")),
  "server.react-server.js",
);

export default defineConfig({
  plugins: [react()],
  resolve: {
    alias: {
      "@": path.join(import.meta.dirname, "src"),
      "next-intl/server": nextIntlServer,
      "next-intl/config": path.join(import.meta.dirname, "src/i18n/request.ts"),
    },
  },
  test: {
    name: "web",
    environment: "node",
    globals: true,
    include: ["test/**/*.test.ts", "test/**/*.test.tsx"],
    server: { deps: { inline: ["next-intl", "use-intl"] } },
  },
});
