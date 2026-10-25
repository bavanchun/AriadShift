// Writes the static export's response headers from route-headers.mjs: out/_headers for
// Netlify, and the nginx server of infra/docker/web.Dockerfile when ARIADSHIFT_WEB_NGINX_CONF
// names a file. Static exports cannot send headers themselves, and each page's CSP pins the
// hashes of the inline scripts in that page, so this runs after `next build`.
// Cloudflare Pages cannot host this export: its 25 MiB file cap excludes pandoc.wasm.
import { readdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { netlifyHeaders, nginxServer, staticHeaderRules } from "../route-headers.mjs";

const out = path.join(import.meta.dirname, "../out");
const pages = readdirSync(out, { recursive: true, encoding: "utf8" })
  .filter((file) => file.endsWith(".html"))
  .map((file) => ({
    file: file.split(path.sep).join("/"),
    html: readFileSync(path.join(out, file), "utf8"),
  }));
const table = staticHeaderRules({ pages, env: process.env });

writeFileSync(path.join(out, "_headers"), netlifyHeaders(table));
const nginx = process.env.ARIADSHIFT_WEB_NGINX_CONF;
if (nginx) {
  writeFileSync(nginx, nginxServer(table));
}
