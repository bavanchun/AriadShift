// Counts the pages of a PDF output by reading its page tree root. Tool results carry no page
// count, and the result card states the outcome in numbers. Only a PDF whose page tree is stored
// in plain objects can be read this way; anything else is `null`, shown as an unknown count.

/** Outputs above this size are not scanned; reading them would cost more than the figure is worth. */
export const PAGE_COUNT_SCAN_LIMIT_BYTES = 32 * 1024 * 1024;

export async function countPdfPages(blob: Blob): Promise<number | null> {
  if (blob.size === 0 || blob.size > PAGE_COUNT_SCAN_LIMIT_BYTES) return null;
  const text = await blob.text();
  if (!text.startsWith("%PDF-")) return null;
  // The root of the page tree is the `/Type /Pages` node without a `/Parent`.
  for (const match of text.matchAll(/\d+ 0 obj([\s\S]*?)endobj/g)) {
    const body = match[1] as string;
    if (!/\/Type\s*\/Pages\b/.test(body) || /\/Parent\b/.test(body)) continue;
    const count = /\/Count\s*(\d+)/.exec(body)?.[1];
    return count ? Number.parseInt(count, 10) : null;
  }
  return null;
}
