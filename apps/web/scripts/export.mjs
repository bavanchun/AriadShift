// Builds a static export into out/ and writes out/_headers for hosts that read it.
import { spawnSync } from "node:child_process";
import { copyFileSync } from "node:fs";
import path from "node:path";

const web = path.join(import.meta.dirname, "..");
const build = spawnSync("pnpm", ["exec", "next", "build"], {
  cwd: web,
  stdio: "inherit",
  shell: process.platform === "win32",
  env: { ...process.env, ARIADSHIFT_WEB_OUTPUT: "export" },
});
if (build.status !== 0) {
  process.exit(build.status ?? 1);
}
await import("./write-static-headers.mjs");
await import("./write-service-worker.mjs");
copyFileSync(path.join(web, "public/sw.js"), path.join(web, "out/sw.js"));
