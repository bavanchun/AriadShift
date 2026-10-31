// Generator configuration for @ariadshift/sdk. `scripts/generate.mjs` and the
// `openapi-ts` CLI both read it. Editing it changes the generated client, so the
// drift check asks for `pnpm --filter @ariadshift/sdk lock` afterwards.
import { fileURLToPath } from "node:url";
import { defineConfig } from "@hey-api/openapi-ts";

/** The committed contract, written by `just openapi` from the server code. */
export const specPath = fileURLToPath(new URL("../../schemas/openapi.v1.json", import.meta.url));

/** Where `generate` writes the client. Not committed. */
export const outputPath = fileURLToPath(new URL("./src/generated", import.meta.url));

/** @param {string} path output directory */
export function sdkConfig(path) {
  return {
    input: specPath,
    output: { path, clean: true },
    logs: { file: false, level: "warn" },
    plugins: ["@hey-api/client-fetch", "@hey-api/typescript", "@hey-api/sdk"],
  };
}

export default defineConfig(sdkConfig(outputPath));
