// Aggregate the official, unmodified Pandoc reactor and its licences as separate files.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { pandocWasmSha256, pandocVersion as version } from "../engine-assets.mjs";
import { renderPandocSourceTxt, verifyPandocSourceInventory } from "../pandoc-source-inventory.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../..");
if (
  !(await readFile(path.join(root, "scripts/install-pandoc.sh"), "utf8")).includes(
    `/download/${version}/`,
  )
)
  throw new Error("Native and browser Pandoc versions must match");
const sha256 = "f14bc3e7722c8bdd58188707e8d5445ce1ac909eae43373f903b45c2f7390cf9";
const dir = path.join(root, ".tools/pandoc-wasm");
await mkdir(dir, { recursive: true });
const archive = path.join(dir, "pandoc.zip");
let bytes;
try {
  bytes = await readFile(archive);
} catch {
  /* First build. */
}
if (!bytes || createHash("sha256").update(bytes).digest("hex") !== sha256) {
  const response = await fetch(
    `https://github.com/jgm/pandoc/releases/download/${version}/pandoc-${version}.wasm.zip`,
  );
  if (!response.ok) throw new Error(`Pandoc download failed: ${response.status}`);
  bytes = Buffer.from(await response.arrayBuffer());
}
if (createHash("sha256").update(bytes).digest("hex") !== sha256)
  throw new Error("Pandoc WASM SHA-256 mismatch");
await writeFile(archive, bytes);
const output = path.join(root, "apps/web/public/wasm/pandoc");
await mkdir(output, { recursive: true });
for (const name of ["pandoc.wasm", "COPYING.md", "COPYRIGHT"]) {
  const data = execFileSync(
    process.platform === "win32" ? "tar" : "unzip",
    [process.platform === "win32" ? "-xOf" : "-p", archive, `pandoc-wasm/${name}`],
    {
      maxBuffer: 70 * 1024 * 1024,
    },
  );
  // The browser checks this digest before it caches or compiles the reactor.
  if (name === "pandoc.wasm") {
    if (createHash("sha256").update(data).digest("hex") !== pandocWasmSha256)
      throw new Error("pandoc.wasm SHA-256 does not match engine-assets.mjs");
    verifyPandocSourceInventory({
      version,
      archiveSha256: sha256,
      wasmSha256: pandocWasmSha256,
      wasmBytes: data,
    });
  }
  await writeFile(path.join(output, name), data);
}
await writeFile(path.join(output, "SOURCE.txt"), renderPandocSourceTxt());
