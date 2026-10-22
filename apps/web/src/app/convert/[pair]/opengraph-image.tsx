import { loadPairs } from "@/lib/catalog";
import { pairSlug, parsePairSlug } from "@/lib/formats";
import { brandImage, OG_SIZE } from "@/lib/og-image";

export const alt = "AriadShift format conversion";
export const size = OG_SIZE;
export const contentType = "image/png";
export const dynamic = "force-static";

export function generateStaticParams() {
  return loadPairs().map(({ from, to }) => ({ pair: pairSlug(from.id, to.id) }));
}

export default async function Image({ params }: { params: Promise<{ pair: string }> }) {
  const pair = parsePairSlug((await params).pair);
  const runsInBrowser = loadPairs().some(
    (candidate) =>
      candidate.from.id === pair?.from.id && candidate.to.id === pair?.to.id && candidate.browser,
  );
  return brandImage(
    pair ? `${pair.from.label} to ${pair.to.label}` : "Document conversion",
    runsInBrowser ? "Converted in your browser, never uploaded" : "Converted on your own computer",
  );
}
