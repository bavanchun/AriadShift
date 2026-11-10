import type { MetadataRoute } from "next";
import { publishedLocales } from "@/i18n/locales";
import { localePath } from "@/i18n/paths";
import { loadPairs } from "@/lib/catalog";
import { pairSlug } from "@/lib/formats";
import { SITE_URL } from "@/lib/site";

export const dynamic = "force-static";

const PAGES = ["/", "/app", "/formats", "/quality", "/download", "/privacy", "/licenses"];

/** One entry per published locale and page, each listing every language of the same page. */
export default function sitemap(): MetadataRoute.Sitemap {
  const locales = publishedLocales();
  const url = (tag: string, path: string) => `${SITE_URL}${localePath(tag, path)}`;
  const entries = (path: string, priority: number): MetadataRoute.Sitemap =>
    locales.map(({ tag }) => ({
      url: url(tag, path),
      priority,
      alternates: {
        languages: {
          ...Object.fromEntries(locales.map((locale) => [locale.tag, url(locale.tag, path)])),
          "x-default": url("en", path),
        },
      },
    }));
  return [
    ...PAGES.flatMap((page) => entries(page, page === "/" ? 1 : 0.8)),
    ...loadPairs().flatMap(({ from, to, browser }) =>
      entries(`/convert/${pairSlug(from.id, to.id)}`, browser ? 0.7 : 0.5),
    ),
  ];
}
