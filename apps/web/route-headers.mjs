// Response headers per route, shared by next.config.ts (`next start`) and
// scripts/write-static-headers.mjs (static export). Cross-origin isolation is limited
// to the converter so embeds and sign-in popups elsewhere keep working.

const isolation = [
  { key: "Cross-Origin-Opener-Policy", value: "same-origin" },
  { key: "Cross-Origin-Embedder-Policy", value: "credentialless" },
];

/** @type {Array<{ source: string; headers: Array<{ key: string; value: string }> }>} */
export const routeHeaders = [
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
