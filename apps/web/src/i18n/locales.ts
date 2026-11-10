// Server-only registry of the locales the site builds, read from locales/locales.toml and
// the catalogues themselves (docs/i18n.md). Nothing here reaches the client bundle: pages
// pass the few values a client component needs as props.
import { existsSync } from "node:fs";
import path from "node:path";
import {
  flatten,
  isPickerVisible,
  measureCompleteness,
  PSEUDO_LOCALES,
  readCatalogue,
  readLocaleMetadata,
  SOURCE_LOCALE,
} from "../../../../scripts/i18n-lib.mjs";

export const DEFAULT_LOCALE = SOURCE_LOCALE;

export interface LocaleInfo {
  tag: string;
  /** Native name for the language picker. */
  name: string;
  dir: "ltr" | "rtl";
  status: "source" | "reviewed" | "community";
  /** Listed in the language picker, the sitemap and the hreflang alternates. */
  published: boolean;
  /** Community text that people may still correct. */
  community: boolean;
}

/** The repository's locales/ directory; builds run in apps/web, root test runs in the root. */
export function localesDirectory(): string {
  const fromWeb = path.join(process.cwd(), "../../locales");
  return existsSync(path.join(fromWeb, "locales.toml"))
    ? fromWeb
    : path.join(process.cwd(), "locales");
}

/** The test-only pseudo-locales are routable only in a build that asks for them. */
export const pseudoLocalesEnabled = () => process.env.ARIADSHIFT_I18N_PSEUDO === "1";

let cache: LocaleInfo[] | null = null;

/** Every locale the build routes, in the order of locales.toml. */
export function allLocales(): LocaleInfo[] {
  if (cache) {
    return cache;
  }
  const dir = localesDirectory();
  const metadata = readLocaleMetadata(dir);
  const source = flatten(readCatalogue(DEFAULT_LOCALE, dir)).messages;
  cache = Object.entries(metadata).map(([tag, entry]) => {
    const { ratio } = measureCompleteness(
      source,
      flatten(readCatalogue(tag, dir)).messages,
      entry.status,
    );
    return {
      tag,
      name: entry.name,
      dir: entry.dir,
      status: entry.status,
      published: isPickerVisible(entry, ratio),
      community: entry.status === "community",
    };
  });
  if (pseudoLocalesEnabled()) {
    for (const [tag, dir] of Object.entries(PSEUDO_LOCALES)) {
      cache.push({
        tag,
        name: tag,
        dir: dir as "ltr" | "rtl",
        status: "community",
        published: false,
        community: true,
      });
    }
  }
  return cache;
}

export const publishedLocales = () => allLocales().filter((locale) => locale.published);

export const findLocale = (tag: string) => allLocales().find((locale) => locale.tag === tag);

export const isLocale = (tag: string) => findLocale(tag) !== undefined;
