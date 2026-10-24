import type { MetadataRoute } from "next";
import { loadPairs } from "@/lib/catalog";
import { pairSlug } from "@/lib/formats";
import { SITE_URL } from "@/lib/site";

export const dynamic = "force-static";

export default function sitemap(): MetadataRoute.Sitemap {
  const pages = ["", "/app", "/formats", "/quality", "/download", "/privacy", "/licenses"];
  return [
    ...pages.map((page) => ({ url: `${SITE_URL}${page}`, priority: page === "" ? 1 : 0.8 })),
    ...loadPairs().map(({ from, to, browser }) => ({
      url: `${SITE_URL}/convert/${pairSlug(from.id, to.id)}`,
      priority: browser ? 0.7 : 0.5,
    })),
  ];
}
