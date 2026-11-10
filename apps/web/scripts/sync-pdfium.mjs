// Aggregate the unmodified PDFium WebAssembly module and its licence texts as separate files.

import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { pdfiumPackageVersion, pdfiumWasmSha256 } from "../pdfium-assets.mjs";

const webDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(path.join(webDir, "package.json"));

// `@embedpdf/pdfium` exports the module as "./pdfium.wasm"; its licence files sit beside the
// package manifest, which the exports map does not expose, so walk up from the entry point.
const wasmPath = require.resolve("@embedpdf/pdfium/pdfium.wasm");
let packageDir = path.dirname(require.resolve("@embedpdf/pdfium"));
while (!existsSync(path.join(packageDir, "package.json"))) packageDir = path.dirname(packageDir);
const manifest = JSON.parse(readFileSync(path.join(packageDir, "package.json"), "utf8"));
if (manifest.version !== pdfiumPackageVersion)
  throw new Error(
    `@embedpdf/pdfium ${manifest.version} is installed but pdfium-assets.mjs pins ${pdfiumPackageVersion}`,
  );
const wasm = readFileSync(wasmPath);
if (createHash("sha256").update(wasm).digest("hex") !== pdfiumWasmSha256)
  throw new Error("pdfium.wasm SHA-256 does not match pdfium-assets.mjs");

const output = path.join(webDir, "public/wasm/pdfium");
mkdirSync(output, { recursive: true });
writeFileSync(path.join(output, "pdfium.wasm"), wasm);
for (const name of ["LICENSE", "LICENSE.pdfium"])
  writeFileSync(path.join(output, name), readFileSync(path.join(packageDir, name)));
