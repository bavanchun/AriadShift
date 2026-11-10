// The same-origin URLs the offline converter may keep in Cache Storage: the /<locale>/app
// shell and static assets. The page that collects URLs (src/converter/offline.ts) and the
// service worker that stores and serves them (scripts/service-worker-source.mjs inlines
// this file) both apply it. Everything else, such as /api/auth/** sessions, /v1/** jobs
// and presigned document URLs, stays out: the Cache API ignores HTTP caching headers, so
// a stored session or JWT would outlive a sign-out on disk.

/** The converter shell: /<locale>/app, where the locale is a BCP 47 tag such as vi or zh-Hans. */
const LOCALE = "[A-Za-z]{2,3}(?:-[A-Za-z0-9]{2,8})*";
const SHELL = new RegExp(`^/${LOCALE}/app$`);
const SHELL_OR_CHILD = new RegExp(`^/${LOCALE}/app(?:/.*)?$`);

/**
 * Whether `pathname` is the converter shell or a page below it.
 * @param {string} pathname
 * @returns {boolean}
 */
export function isConverterPath(pathname) {
  return SHELL_OR_CHILD.test(pathname);
}

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
  if (SHELL.test(url.pathname)) {
    return url.search === "";
  }
  return (
    STATIC_PREFIXES.some((prefix) => url.pathname.startsWith(prefix)) ||
    ROOT_ICONS.includes(url.pathname)
  );
}
