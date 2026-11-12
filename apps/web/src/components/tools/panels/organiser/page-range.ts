// Client-side expansion of the shared page-range grammar (`docs/tools.md` §5), used only for
// previews and counts. Validation of the text itself stays with `validatePageRange`.

const REF = /^(?:[1-9]\d*|last(?:-[1-9]\d*)?)$/;
const RANGE = /^([1-9]\d*|last(?:-[1-9]\d*)?)?-([1-9]\d*|last(?:-[1-9]\d*)?)?$/;

function resolveRef(ref: string, pageCount: number): number | null {
  if (!REF.test(ref)) return null;
  const page =
    ref === "last"
      ? pageCount
      : ref.startsWith("last-")
        ? pageCount - Number(ref.slice(5))
        : Number(ref);
  return page >= 1 && page <= pageCount ? page : null;
}

/**
 * Expands a page-range selector into 1-based pages in selection order, duplicates kept.
 * Returns null when the text is not a valid selector for a document of `pageCount` pages.
 */
export function expandPageRange(value: string, pageCount: number): number[] | null {
  if (!Number.isInteger(pageCount) || pageCount < 1 || !value.trim()) return null;
  const pages: number[] = [];
  for (const raw of value.split(",")) {
    const token = raw.trim();
    if (token === "all") for (let page = 1; page <= pageCount; page++) pages.push(page);
    else if (token === "odd") for (let page = 1; page <= pageCount; page += 2) pages.push(page);
    else if (token === "even") for (let page = 2; page <= pageCount; page += 2) pages.push(page);
    else if (REF.test(token)) {
      const page = resolveRef(token, pageCount);
      if (page === null) return null;
      pages.push(page);
    } else {
      const match = token.match(RANGE);
      if (!match || (!match[1] && !match[2])) return null;
      const start = match[1] ? resolveRef(match[1], pageCount) : 1;
      const end = match[2] ? resolveRef(match[2], pageCount) : pageCount;
      if (start === null || end === null) return null;
      const step = start <= end ? 1 : -1;
      for (let page = start; page !== end + step; page += step) pages.push(page);
    }
  }
  return pages;
}

/** Number of distinct pages a selector picks, or null when it is not valid for the document. */
export function countSelectedPages(value: string, pageCount: number): number | null {
  const pages = expandPageRange(value, pageCount);
  return pages ? new Set(pages).size : null;
}

export const hasDuplicates = (pages: readonly number[]): boolean =>
  new Set(pages).size !== pages.length;
