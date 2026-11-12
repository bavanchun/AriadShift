// Redirects from the unprefixed paths used before the site moved under /<locale>/. Shared by
// next.config.ts (`next start`) and scripts/write-static-headers.mjs (static export, which
// writes them to out/_redirects). Old links land on the default locale, permanently.

const DEFAULT_LOCALE = "en";

/** @type {Array<{ source: string; destination: string }>} */
export const routeRedirects = [
  { source: "/app", destination: `/${DEFAULT_LOCALE}/app` },
  { source: "/app/:path*", destination: `/${DEFAULT_LOCALE}/app/:path*` },
  { source: "/convert/:pair", destination: `/${DEFAULT_LOCALE}/convert/:pair` },
  { source: "/formats", destination: `/${DEFAULT_LOCALE}/formats` },
  { source: "/quality", destination: `/${DEFAULT_LOCALE}/quality` },
  { source: "/privacy", destination: `/${DEFAULT_LOCALE}/privacy` },
  { source: "/licenses", destination: `/${DEFAULT_LOCALE}/licenses` },
  { source: "/download", destination: `/${DEFAULT_LOCALE}/download` },
  { source: "/login", destination: `/${DEFAULT_LOCALE}/sign-in` },
  { source: "/sign-in", destination: `/${DEFAULT_LOCALE}/sign-in` },
  { source: "/account", destination: `/${DEFAULT_LOCALE}/account` },
  { source: "/account/:path*", destination: `/${DEFAULT_LOCALE}/account/:path*` },
  { source: "/api-keys", destination: `/${DEFAULT_LOCALE}/api-keys` },
  { source: "/jobs", destination: `/${DEFAULT_LOCALE}/jobs` },
];

/** Status for permanent redirects that keep the request method. */
export const REDIRECT_STATUS = 308;
