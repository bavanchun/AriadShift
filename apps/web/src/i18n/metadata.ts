import type { Metadata } from "next";
import { publishedLocales } from "./locales";
import { localePath } from "./paths";

/** Canonical URL plus hreflang alternates for a page, listing only published locales. */
export function pageAlternates(locale: string, path: string): Metadata["alternates"] {
  const languages: Record<string, string> = {};
  for (const { tag } of publishedLocales()) {
    languages[tag] = localePath(tag, path);
  }
  languages["x-default"] = localePath("en", path);
  return { canonical: localePath(locale, path), languages };
}
