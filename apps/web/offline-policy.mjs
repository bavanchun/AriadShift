// The same-origin URLs the offline converter may keep in Cache Storage: the /app shell
// and static assets. The page that collects URLs (src/converter/offline.ts) and the
// service worker that stores and serves them (scripts/service-worker-source.mjs inlines
// this file) both apply it. Everything else, such as /api/auth/** sessions, /v1/** jobs
// and presigned document URLs, stays out: the Cache API ignores HTTP caching headers, so
// a stored session or JWT would outlive a sign-out on disk.

/** Hashed build output, engine binaries and brand images. */
const STATIC_PREFIXES = ["/_next/static/", "/wasm/", "/brand/"];
/** Metadata icons served at the root, with a content hash in the query. */
const ROOT_ICONS = ["/icon.svg", "/apple-icon.png", "/favicon.ico"];

/**
 * Whether `url` belongs to the offline converter.
 * @param {URL} url
 * @param {string} origin the site's origin
 * @returns {boolean}
 */
export function isOfflineAsset(url, origin) {
  if (!["http:", "https:"].includes(url.protocol) || url.origin !== origin) {
    return false;
  }
  if (url.pathname === "/app") {
    return url.search === "";
  }
  return (
    STATIC_PREFIXES.some((prefix) => url.pathname.startsWith(prefix)) ||
    ROOT_ICONS.includes(url.pathname)
  );
}
