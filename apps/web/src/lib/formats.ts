/** Display information for the formats the site talks about, keyed by registry id. */
export interface FormatInfo {
  id: string;
  /** URL segment used in /convert/<from>-to-<to>. */
  slug: string;
  label: string;
  /** Short noun phrase, for example "Markdown document". */
  noun: string;
  extensions: string[];
  description: string;
}

export const FORMATS: FormatInfo[] = [
  {
    id: "markdown",
    slug: "markdown",
    label: "Markdown",
    noun: "Markdown document",
    extensions: ["md", "markdown"],
    description:
      "CommonMark with GitHub Flavored Markdown: headings, lists, tables, task lists, footnotes and math. The format most AI tools read best.",
  },
  {
    id: "html",
    slug: "html",
    label: "HTML",
    noun: "HTML page",
    extensions: ["html", "htm"],
    description:
      "A standalone HTML5 page. Raw markup in the input is sanitized; scripts and unsafe attributes never reach the output.",
  },
  {
    id: "ariad-ir+json",
    slug: "ir-json",
    label: "AriadShift IR (JSON)",
    noun: "AriadShift IR document",
    extensions: ["ir.json", "json"],
    description:
      "The structured document model every AriadShift route passes through: headings, blocks, tables, assets and metadata as versioned JSON.",
  },
  {
    id: "docx",
    slug: "docx",
    label: "Word (DOCX)",
    noun: "Word document",
    extensions: ["docx"],
    description:
      "Office Open XML documents from Microsoft Word, LibreOffice and Google Docs, read and written through Pandoc.",
  },
  {
    id: "epub",
    slug: "epub",
    label: "EPUB",
    noun: "EPUB e-book",
    extensions: ["epub"],
    description: "EPUB 3 e-books, read and written through Pandoc with reproducible metadata.",
  },
];

const BY_ID = new Map(FORMATS.map((format) => [format.id, format]));
const BY_SLUG = new Map(FORMATS.map((format) => [format.slug, format]));

export function formatById(id: string): FormatInfo | undefined {
  return BY_ID.get(id);
}

/** A readable label for any registry id, including formats without display information. */
export function formatLabel(id: string): string {
  return BY_ID.get(id)?.label ?? id;
}

export function pairSlug(from: string, to: string): string {
  return `${BY_ID.get(from)?.slug ?? from}-to-${BY_ID.get(to)?.slug ?? to}`;
}

export function parsePairSlug(slug: string): { from: FormatInfo; to: FormatInfo } | undefined {
  const [fromSlug, toSlug, ...rest] = slug.split("-to-");
  if (fromSlug === undefined || toSlug === undefined || rest.length > 0) {
    return undefined;
  }
  const from = BY_SLUG.get(fromSlug);
  const to = BY_SLUG.get(toSlug);
  return from && to && from !== to ? { from, to } : undefined;
}

/** File picker `accept` value covering every format with display information. */
export const ACCEPT = FORMATS.flatMap((format) =>
  format.extensions.filter((extension) => !extension.includes(".")).map((ext) => `.${ext}`),
).join(",");
