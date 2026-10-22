// Writes out/_headers (Netlify and Cloudflare Pages format) from the shared header
// table, because a static export cannot send response headers itself.
import { writeFileSync } from "node:fs";
import path from "node:path";
import { routeHeaders } from "../route-headers.mjs";

const toPattern = (source) => source.replace(/\/:path\*$/, "/*");

const lines = routeHeaders.flatMap(({ source, headers }) => [
  toPattern(source),
  ...headers.map(({ key, value }) => `  ${key}: ${value}`),
  "",
]);

writeFileSync(path.join(import.meta.dirname, "../out/_headers"), lines.join("\n"));
