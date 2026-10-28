// Builds the service worker script: this build's engine manifest, the offline cache
// policy and the worker itself. The worker is a classic script, so the policy's
// `export` keywords are dropped when it is inlined.
import { readFile } from "node:fs/promises";
import path from "node:path";

const web = path.resolve(import.meta.dirname, "..");

/** @param {string[]} engineUrls versioned binaries and hashed chunks of this build */
export async function serviceWorkerSource(engineUrls) {
  const policy = await readFile(path.join(web, "offline-policy.mjs"), "utf8");
  const worker = await readFile(path.join(web, "src/converter/service-worker.js"), "utf8");
  return [
    `self.ENGINE_URLS = ${JSON.stringify(engineUrls)};`,
    policy.replace(/^export /gm, ""),
    worker,
  ].join("\n");
}
