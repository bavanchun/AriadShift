// Writes Netlify's out/_headers and out/_redirects from the shared tables; static exports
// cannot send headers or redirects. Cloudflare Pages cannot host this export: its 25 MiB
// file cap excludes pandoc.wasm. `:locale` expands to every built locale, so each rule
// names a concrete path.
import { readdirSync, statSync, writeFileSync } from "node:fs";
import path from "node:path";
import { routeHeaders } from "../route-headers.mjs";
import { REDIRECT_STATUS, routeRedirects } from "../route-redirects.mjs";

const out = path.join(import.meta.dirname, "../out");

/** Locale directories of the export: top-level folders that contain an app/ page. */
export function builtLocales(directory = out) {
  return readdirSync(directory)
    .filter((name) => {
      try {
        return statSync(path.join(directory, name, "app")).isDirectory();
      } catch {
        return false;
      }
    })
    .sort();
}

const toPattern = (source) => source.replace(/\/:path\*$/, "/*");

export function headersFile(locales) {
  const rules = routeHeaders.flatMap((rule) =>
    rule.source.includes(":locale")
      ? locales.map((locale) => ({ ...rule, source: rule.source.replace(":locale", locale) }))
      : [rule],
  );
  return [
    "# Netlify headers. Cloudflare Pages is unsupported: pandoc.wasm exceeds its 25 MiB file cap.",
    ...rules.flatMap(({ source, headers }) => [
      toPattern(source),
      ...headers.map(({ key, value }) => `  ${key}: ${value}`),
      "",
    ]),
  ].join("\n");
}

export function redirectsFile() {
  return [
    "# Netlify redirects: paths without a locale prefix move to the default locale.",
    ...routeRedirects.map(
      ({ source, destination }) =>
        `${toPattern(source)} ${destination.replace(/\/:path\*$/, "/:splat")} ${REDIRECT_STATUS}`,
    ),
    "",
  ].join("\n");
}

/** Writes out/_headers and out/_redirects for the finished export. */
export function writeStaticRouting(directory = out) {
  writeFileSync(path.join(directory, "_headers"), headersFile(builtLocales(directory)));
  writeFileSync(path.join(directory, "_redirects"), redirectsFile());
}
