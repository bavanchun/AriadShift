// Response headers per route: the single source for next.config.ts (`next start` and
// `next dev`), scripts/write-static-headers.mjs (Netlify `_headers` and the nginx server
// of infra/docker/web.Dockerfile) and the header tests. docs/web.md ("Security headers")
// explains every directive.
//
// A static export cannot carry a per-request nonce, so the export pins each page's inline
// scripts by SHA-256. `next start` cannot hash pages that do not exist yet when Next.js
// compiles its header table, so it allows inline scripts instead; everything else is the
// same policy. Cross-origin isolation is limited to the converter so embeds and sign-in
// popups elsewhere keep working.
import { createHash } from "node:crypto";

/** @typedef {{ key: string; value: string }} Header */
/** @typedef {{ source: string; headers: Header[] }} RouteHeaders */
/** @typedef {"site" | "converter"} PageProfile */
/**
 * Extra sources the converter talks to in cloud mode, from build-time configuration.
 * @typedef {{ connect: string[]; script: string[]; frame: string[] }} CloudSources
 */
/**
 * @typedef {object} PolicyOptions
 * @property {string[] | "unsafe-inline"} inlineScripts `'sha256-…'` sources of the page's
 *   inline scripts, or `"unsafe-inline"` where they cannot be hashed (`next start`).
 * @property {CloudSources} [cloud] Cloud mode origins; converter pages only.
 * @property {boolean} [dev] `next dev`: React needs `'unsafe-eval'` for its debugging aids.
 */

const TURNSTILE_ORIGIN = "https://challenges.cloudflare.com";

/** Headers every response carries. */
export const commonHeaders = [
  // Cross-origin requests see the origin only, never the path or the query (`/app?from=…`).
  { key: "Referrer-Policy", value: "strict-origin-when-cross-origin" },
  { key: "X-Content-Type-Options", value: "nosniff" },
  // Powerful features the site never uses stay off for it and anything it embeds.
  {
    key: "Permissions-Policy",
    value:
      "camera=(), microphone=(), geolocation=(), payment=(), usb=(), serial=(), hid=(), browsing-topics=()",
  },
];

/**
 * HSTS for hosts that terminate TLS themselves (Netlify). Behind a TLS proxy, the proxy
 * sends it: browsers ignore the header on plain HTTP. No `includeSubDomains` or `preload`,
 * which bind the operator's whole domain and are hard to undo.
 */
export const strictTransportSecurity = {
  key: "Strict-Transport-Security",
  value: "max-age=31536000",
};

const isolation = [
  { key: "Cross-Origin-Opener-Policy", value: "same-origin" },
  { key: "Cross-Origin-Embedder-Policy", value: "credentialless" },
];

// A cross-origin-isolated page may only start a worker whose script declares a compatible
// embedder policy. A worker started from a URL does not inherit the page's CSP but takes
// the one sent with its script, so worker chunks and the service worker get their own:
// scripts from this origin and WebAssembly compilation only. On a chunk loaded by a page
// these headers have no effect.
const workerPolicy = "script-src 'self' 'wasm-unsafe-eval'; object-src 'none'; base-uri 'none'";
const chunkHeaders = [
  { key: "Cross-Origin-Embedder-Policy", value: "credentialless" },
  { key: "Content-Security-Policy", value: workerPolicy },
];
const serviceWorkerHeaders = [{ key: "Content-Security-Policy", value: workerPolicy }];

/** Whether a route path is the converter, which runs the engines and cloud mode. */
export function isConverterPath(/** @type {string} */ route) {
  return route === "/app" || route === "/app.html" || route.startsWith("/app/");
}

/**
 * The Content-Security-Policy of one page.
 * @param {PageProfile} profile
 * @param {PolicyOptions} options
 * @returns {string}
 */
export function contentSecurityPolicy(profile, { inlineScripts, cloud, dev = false }) {
  const converter = profile === "converter";
  const extra = converter && cloud ? cloud : { connect: [], script: [], frame: [] };
  const inline =
    inlineScripts === "unsafe-inline"
      ? ["'unsafe-inline'"]
      : [...new Set(inlineScripts)].sort().map((hash) => `'${hash}'`);
  /** @type {Array<[string, string[]]>} */
  const directives = [
    ["default-src", ["'self'"]],
    [
      "script-src",
      [
        "'self'",
        ...inline,
        // The engines compile WebAssembly; `'unsafe-eval'` (JavaScript eval) stays off.
        ...(converter ? ["'wasm-unsafe-eval'"] : []),
        ...(dev ? ["'unsafe-eval'"] : []),
        ...extra.script,
      ],
    ],
    // React renders `style` attributes, which cannot be hashed without 'unsafe-hashes'.
    ["style-src", ["'self'", "'unsafe-inline'"]],
    ["img-src", ["'self'"]],
    ["font-src", ["'self'"]],
    ["connect-src", ["'self'", ...extra.connect]],
    ["worker-src", converter ? ["'self'"] : ["'none'"]],
    ["frame-src", extra.frame.length > 0 ? extra.frame : ["'none'"]],
    ["object-src", ["'none'"]],
    ["base-uri", ["'none'"]],
    ["form-action", ["'self'"]],
    ["frame-ancestors", ["'none'"]],
  ];
  return directives.map(([name, sources]) => `${name} ${sources.join(" ")}`).join("; ");
}

// `scheme://host[:port]`, optionally with a leading `*.` label, as CSP host sources take.
const ORIGIN_SOURCE = /^https?:\/\/(?:\*\.)?[a-z0-9-]+(?:\.[a-z0-9-]+)*(?::\d{1,5})?$/;

/**
 * The origin of a configured base URL as a CSP source; `null` when unset.
 * @param {string | undefined} value
 * @param {string} name
 */
function originOf(value, name) {
  const trimmed = value?.trim();
  if (!trimmed) {
    return null;
  }
  if (!URL.canParse(trimmed)) {
    throw new Error(`${name} is not a URL: ${trimmed}`);
  }
  const origin = new URL(trimmed).origin;
  if (!ORIGIN_SOURCE.test(origin)) {
    throw new Error(`${name} must be an http or https URL: ${trimmed}`);
  }
  return origin;
}

/**
 * Cloud mode's sources from the same build-time variables the app reads
 * (src/lib/cloud/config.ts). Without an API URL the site has no cloud mode and the
 * converter talks to its own origin only. `ARIADSHIFT_WEB_STORAGE_ORIGINS` lists the
 * object-store origins of the API's presigned URLs, separated by spaces; a virtual-hosted
 * bucket may use a `https://*.` wildcard. A malformed value fails the build rather than
 * shipping a policy that blocks uploads or allows more than intended.
 * @param {Record<string, string | undefined>} env
 * @returns {CloudSources}
 */
export function cloudSources(env) {
  const api = originOf(env.NEXT_PUBLIC_ARIAD_API_URL, "NEXT_PUBLIC_ARIAD_API_URL");
  if (!api) {
    return { connect: [], script: [], frame: [] };
  }
  const auth = originOf(env.NEXT_PUBLIC_ARIAD_AUTH_URL, "NEXT_PUBLIC_ARIAD_AUTH_URL");
  const storage = (env.ARIADSHIFT_WEB_STORAGE_ORIGINS ?? "").split(/\s+/).filter(Boolean);
  for (const source of storage) {
    if (!ORIGIN_SOURCE.test(source)) {
      throw new Error(`ARIADSHIFT_WEB_STORAGE_ORIGINS takes http or https origins: ${source}`);
    }
  }
  const turnstile = Boolean(env.NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY?.trim());
  return {
    connect: [...new Set([api, ...(auth ? [auth] : []), ...storage])],
    script: turnstile ? [TURNSTILE_ORIGIN] : [],
    frame: turnstile ? [TURNSTILE_ORIGIN] : [],
  };
}

/**
 * `'sha256-…'` sources for every inline `<script>` in an exported page. Script blocks
 * with a `src` are covered by `'self'`; data blocks (`type="application/json"`) are hashed
 * too, which is harmless.
 * @param {string} html
 * @returns {string[]}
 */
export function inlineScriptHashes(html) {
  const hashes = new Set();
  for (const [, attributes, body] of html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi)) {
    if (/\ssrc\s*=/i.test(` ${attributes}`)) {
      continue;
    }
    // The HTML parser turns CR and CRLF into LF before the browser hashes the text.
    const text = (body ?? "").replace(/\r\n?/g, "\n");
    hashes.add(`sha256-${createHash("sha256").update(text, "utf8").digest("base64")}`);
  }
  return [...hashes].sort();
}

/**
 * The header table for `next start` and `next dev`. Entries apply in order and a later
 * entry overrides a header an earlier one set, so the catch-all comes first.
 * @param {{ env: Record<string, string | undefined>; dev?: boolean }} options
 * @returns {RouteHeaders[]}
 */
export function serverRouteHeaders({ env, dev = false }) {
  const cloud = cloudSources(env);
  const policy = (/** @type {PageProfile} */ profile) => ({
    key: "Content-Security-Policy",
    value: contentSecurityPolicy(profile, { inlineScripts: "unsafe-inline", cloud, dev }),
  });
  const converter = [...isolation, policy("converter")];
  return [
    { source: "/:path*", headers: [...commonHeaders, policy("site")] },
    { source: "/app", headers: converter },
    { source: "/app/:path*", headers: converter },
    { source: "/_next/static/chunks/:path*", headers: chunkHeaders },
    { source: "/sw.js", headers: serviceWorkerHeaders },
  ];
}

/** The table next.config.ts serves, from this build's environment. */
export const routeHeaders = serverRouteHeaders({
  env: process.env,
  dev: process.env.NODE_ENV === "development",
});

/**
 * The URL paths that serve one exported HTML file, relative to the export root.
 * @param {string} file for example `index.html`, `app.html` or `convert/a-to-b.html`
 * @returns {string[]}
 */
export function pagePaths(file) {
  const path = `/${file.replace(/\\/g, "/").replace(/^\/+/, "")}`;
  if (path.endsWith("/index.html")) {
    return [path.slice(0, -"index.html".length), path];
  }
  return [path.slice(0, -".html".length), path];
}

/**
 * Header rules for a static export: one rule per exported page with its own hashes (the
 * converter's pages also isolated), the worker scripts and the headers every response
 * carries. No two rules set the same header on one path, because Netlify joins the values
 * of every matching rule: two joined CSPs would both apply, and a joined COOP is invalid.
 * @param {{ pages: Array<{ file: string; html: string }>; env: Record<string, string | undefined> }} options
 * @returns {{ common: Header[]; rules: RouteHeaders[] }}
 */
export function staticHeaderRules({ pages, env }) {
  const cloud = cloudSources(env);
  /** @type {RouteHeaders[]} */
  const rules = [];
  for (const { file, html } of [...pages].sort((a, b) => a.file.localeCompare(b.file))) {
    const paths = pagePaths(file);
    const profile = paths.some(isConverterPath) ? "converter" : "site";
    const policy = contentSecurityPolicy(profile, {
      inlineScripts: inlineScriptHashes(html),
      cloud,
    });
    for (const source of paths) {
      rules.push({
        source,
        headers: [
          ...(profile === "converter" ? isolation : []),
          { key: "Content-Security-Policy", value: policy },
        ],
      });
    }
  }
  rules.push({ source: "/_next/static/chunks/*", headers: chunkHeaders });
  rules.push({ source: "/sw.js", headers: serviceWorkerHeaders });
  return { common: commonHeaders, rules };
}

/**
 * Netlify's `_headers` file. Netlify always serves HTTPS, so it also sends HSTS.
 * @param {{ common: Header[]; rules: RouteHeaders[] }} table
 */
export function netlifyHeaders({ common, rules }) {
  const block = (/** @type {string} */ source, /** @type {Header[]} */ headers) => [
    source,
    ...headers.map(({ key, value }) => `  ${key}: ${value}`),
    "",
  ];
  return [
    "# Generated by apps/web/scripts/write-static-headers.mjs from apps/web/route-headers.mjs.",
    "# Cloudflare Pages is unsupported: pandoc.wasm exceeds its 25 MiB file cap.",
    ...block("/*", [...common, strictTransportSecurity]),
    ...rules.flatMap(({ source, headers }) => block(source, headers)),
  ].join("\n");
}

/** @param {Header} header */
function nginxHeader({ key, value }) {
  if (/["\\$\r\n]/.test(value) || !/^[A-Za-z-]+$/.test(key)) {
    throw new Error(`header cannot be written to nginx: ${key}`);
  }
  return `        add_header ${key} "${value}" always;`;
}

/**
 * The nginx server for the static export (infra/docker/web.Dockerfile). nginx drops every
 * inherited `add_header` in a location that sets its own, so each location repeats the
 * common headers. Pages get exact-match locations; HSTS belongs to the TLS proxy in front.
 * @param {{ common: Header[]; rules: RouteHeaders[] }} table
 * @param {{ port?: number; root?: string }} [server]
 */
export function nginxServer({ common, rules }, { port = 3000, root = "/srv/web" } = {}) {
  const location = (
    /** @type {string} */ match,
    /** @type {Header[]} */ headers,
    /** @type {string} */ tryFiles,
  ) => [
    `    location ${match} {`,
    ...[...common, ...headers].map(nginxHeader),
    `        try_files ${tryFiles};`,
    "    }",
  ];
  const pages = rules.filter(({ source }) => source.endsWith(".html"));
  const pageLocations = pages.flatMap(({ source, headers }) => {
    const pretty = pagePaths(source.slice(1))[0] ?? source;
    return [
      ...location(`= ${pretty}`, headers, `${source} =404`),
      ...location(`= ${source}`, headers, `${source} =404`),
    ];
  });
  const prefix = (/** @type {string} */ source) =>
    rules.find((rule) => rule.source === source)?.headers ?? [];
  return [
    "# Generated by apps/web/scripts/write-static-headers.mjs from apps/web/route-headers.mjs.",
    "server {",
    `    listen ${port};`,
    "    server_name _;",
    "    server_tokens off;",
    "    absolute_redirect off;",
    `    root ${root};`,
    "    error_page 404 /404.html;",
    "",
    "    gzip on;",
    "    gzip_types application/javascript application/json application/wasm image/svg+xml text/css text/plain;",
    "",
    // Requests that match no page location (assets, 404s) get the common headers only.
    ...location("/", [], "$uri $uri.html $uri/ =404"),
    ...location("^~ /_next/static/chunks/", prefix("/_next/static/chunks/*"), "$uri =404"),
    ...location("= /sw.js", prefix("/sw.js"), "$uri =404"),
    ...pageLocations,
    "}",
    "",
  ].join("\n");
}
