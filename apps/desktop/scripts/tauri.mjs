import { spawnSync } from "node:child_process";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";

export function signingInvocation(args, environment, platform = process.platform) {
  const env = Object.fromEntries(
    Object.entries(environment).filter(
      ([name, value]) => !name.startsWith("APPLE_") || value !== "",
    ),
  );
  const next = [...args];
  if (platform === "darwin" && ["build", "bundle"].includes(next[0])) {
    if (!env.APPLE_CERTIFICATE) {
      if (!next.includes("--no-sign")) next.push("--no-sign");
    } else {
      for (const name of ["APPLE_CERTIFICATE_PASSWORD", "APPLE_SIGNING_IDENTITY"]) {
        if (!env[name]) throw new Error(`Apple signing requires ${name} with APPLE_CERTIFICATE`);
      }
      const notarization = ["APPLE_ID", "APPLE_PASSWORD", "APPLE_TEAM_ID"];
      if (notarization.some((name) => env[name]) && !notarization.every((name) => env[name])) {
        throw new Error(
          "Apple notarization requires APPLE_ID, APPLE_PASSWORD and APPLE_TEAM_ID together",
        );
      }
    }
  }
  return { args: next, env };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const invocation = signingInvocation(process.argv.slice(2), process.env);
  const cli = createRequire(import.meta.url).resolve("@tauri-apps/cli/tauri.js");
  const child = spawnSync(process.execPath, [cli, ...invocation.args], {
    env: invocation.env,
    stdio: "inherit",
  });
  if (child.error) throw child.error;
  process.exit(child.status ?? 1);
}
