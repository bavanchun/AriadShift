// Response headers per route, shared by next.config.ts (`next start`) and
// scripts/write-static-headers.mjs (static export). Cross-origin isolation is limited
// to the converter so embeds and sign-in popups elsewhere keep working. Every page lives
// under /<locale>/, so the converter is /:locale/app; the unprefixed /app only redirects.

const isolation = [
  { key: "Cross-Origin-Opener-Policy", value: "same-origin" },
  { key: "Cross-Origin-Embedder-Policy", value: "credentialless" },
];

/** @type {Array<{ source: string; headers: Array<{ key: string; value: string }> }>} */
export const routeHeaders = [
  { source: "/:locale/app", headers: isolation },
  { source: "/:locale/app/:path*", headers: isolation },
  // Compatibility: a host that serves the old path directly stays isolated too.
  { source: "/app", headers: isolation },
  { source: "/app/:path*", headers: isolation },
  // A cross-origin-isolated page may only start a worker whose script declares a
  // compatible embedder policy. Chunk names are hashed, so every chunk carries it; the
  // header has no effect on ordinary scripts.
  {
    source: "/_next/static/chunks/:path*",
    headers: [{ key: "Cross-Origin-Embedder-Policy", value: "credentialless" }],
  },
];
