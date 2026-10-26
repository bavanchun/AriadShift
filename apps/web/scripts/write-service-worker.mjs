// Embed the current deployment's assets so activation retires old engine downloads.
import { readdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { pandocUrl } from "../engine-assets.mjs";

const web = path.resolve(import.meta.dirname, "..");
const urls = process.argv.includes("--dev")
  ? []
  : [
      pandocUrl,
      ...(await readdir(path.join(web, ".next/static"), { recursive: true }))
        .filter((file) => /\.(?:js|wasm)$/.test(file))
        .map((file) => `/_next/static/${file.split(path.sep).join("/")}`),
    ].sort();
const source = await readFile(path.join(web, "src/converter/service-worker.js"), "utf8");
await writeFile(
  path.join(web, "public/sw.js"),
  `self.ENGINE_URLS = ${JSON.stringify(urls)};\n${source}`,
);
