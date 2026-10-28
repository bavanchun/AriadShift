// Embed the current deployment's assets so activation retires old engine downloads.
import { readdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { pandocUrl } from "../engine-assets.mjs";
import { serviceWorkerSource } from "./service-worker-source.mjs";

const web = path.resolve(import.meta.dirname, "..");
const urls = process.argv.includes("--dev")
  ? []
  : [
      pandocUrl,
      ...(await readdir(path.join(web, ".next/static"), { recursive: true }))
        .filter((file) => /\.(?:js|wasm)$/.test(file))
        .map((file) => `/_next/static/${file.split(path.sep).join("/")}`),
    ].sort();
await writeFile(path.join(web, "public/sw.js"), await serviceWorkerSource(urls));
