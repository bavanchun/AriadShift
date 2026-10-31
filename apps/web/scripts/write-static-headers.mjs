// Writes Netlify's out/_headers from the shared table; static exports cannot send headers.
// Cloudflare Pages cannot host this export: its 25 MiB file cap excludes pandoc.wasm.
import { writeFileSync } from "node:fs";
import path from "node:path";
import { routeHeaders } from "../route-headers.mjs";

const toPattern = (source) => source.replace(/\/:path\*$/, "/*");

const lines = [
  "# Netlify headers. Cloudflare Pages is unsupported: pandoc.wasm exceeds its 25 MiB file cap.",
  ...routeHeaders.flatMap(({ source, headers }) => [
    toPattern(source),
    ...headers.map(({ key, value }) => `  ${key}: ${value}`),
    "",
  ]),
];

writeFileSync(path.join(import.meta.dirname, "../out/_headers"), lines.join("\n"));
