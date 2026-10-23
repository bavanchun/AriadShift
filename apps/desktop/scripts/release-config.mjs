import { writeFileSync } from "node:fs";

const tag = process.env.RELEASE_TAG ?? "";
if (!/^v\d+\.\d+\.\d+$/.test(tag)) throw new Error("A stable vX.Y.Z release tag is required");
writeFileSync(
  new URL("../src-tauri/tauri.release.local.json", import.meta.url),
  `${JSON.stringify({ version: tag.slice(1), bundle: { createUpdaterArtifacts: true } }, null, 2)}\n`,
);
