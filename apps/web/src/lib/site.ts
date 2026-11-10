export const SITE_URL = "https://ariadshift.ariadnev.com";
export const SITE_NAME = "AriadShift";
export const REPOSITORY_URL = "https://github.com/bavanchun/AriadShift";

/** Main navigation; the label of each entry is the `web.shell.nav.<key>` message. */
export const NAVIGATION = [
  { href: "/app", key: "converter" },
  { href: "/formats", key: "formats" },
  { href: "/quality", key: "quality" },
  { href: "/download", key: "download" },
  { href: "/privacy", key: "privacy" },
] as const;
