// Generates the typed /v1 client into src/generated/ from schemas/openapi.v1.json.
import { pathToFileURL } from "node:url";
import { createClient } from "@hey-api/openapi-ts";
import { outputPath, sdkConfig } from "../openapi-ts.config.mjs";

/** Writes the client to `path` (src/generated/ by default). */
export async function generate(path = outputPath) {
  await createClient(sdkConfig(path));
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  await generate();
}
