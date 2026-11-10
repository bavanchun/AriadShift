/**
 * Display information for the formats the site talks about, keyed by registry id. Names and
 * extensions are fixed; the descriptive prose is `common.format.<key>.noun` and
 * `.description` in the message catalogue (see `formatMessageKey`).
 */
export interface FormatInfo {
  id: string;
  /** URL segment used in /convert/<from>-to-<to>. */
  slug: string;
  label: string;
  extensions: string[];
  /** Internal engine interchange retains labels but has no public format page. */
  internal?: boolean;
}

export const FORMATS: FormatInfo[] = [
  {
    id: "markdown",
    slug: "markdown",
    label: "Markdown",
    extensions: ["md", "markdown", "mdown"],
  },
  {
    id: "html",
    slug: "html",
    label: "HTML",
    extensions: ["html", "htm"],
  },
  {
    id: "ariad-ir+json",
    slug: "ir-json",
    label: "AriadShift IR (JSON)",
    extensions: ["ir.json", "json"],
  },
  {
    id: "docx",
    slug: "docx",
    label: "Word (DOCX)",
    extensions: ["docx"],
  },
  {
    id: "epub",
    slug: "epub",
    label: "EPUB",
    extensions: ["epub"],
  },
  {
    id: "pdf",
    slug: "pdf",
    label: "PDF",
    extensions: ["pdf"],
  },
  {
    id: "odt",
    slug: "odt",
    label: "OpenDocument Text (ODT)",
    extensions: ["odt"],
  },
  {
    id: "rtf",
    slug: "rtf",
    label: "Rich Text (RTF)",
    extensions: ["rtf"],
  },
  {
    id: "pptx",
    slug: "pptx",
    label: "PowerPoint (PPTX)",
    extensions: ["pptx"],
  },
  {
    id: "xlsx",
    slug: "xlsx",
    label: "Excel (XLSX)",
    extensions: ["xlsx"],
  },
  {
    id: "doc",
    slug: "doc",
    label: "Word (DOC)",
    extensions: ["doc"],
  },
  {
    id: "xls",
    slug: "xls",
    label: "Excel (XLS)",
    extensions: ["xls"],
  },
  {
    id: "ppt",
    slug: "ppt",
    label: "PowerPoint (PPT)",
    extensions: ["ppt"],
  },
  {
    id: "ods",
    slug: "ods",
    label: "OpenDocument Spreadsheet (ODS)",
    extensions: ["ods"],
  },
  {
    id: "odp",
    slug: "odp",
    label: "OpenDocument Presentation (ODP)",
    extensions: ["odp"],
  },
  {
    id: "png",
    slug: "png",
    label: "PNG",
    extensions: ["png"],
  },
  {
    id: "jpeg",
    slug: "jpeg",
    label: "JPEG",
    extensions: ["jpg", "jpeg"],
  },
  {
    id: "webp",
    slug: "webp",
    label: "WebP",
    extensions: ["webp"],
  },
  {
    id: "tiff",
    slug: "tiff",
    label: "TIFF",
    extensions: ["tif", "tiff"],
  },
  {
    id: "txt",
    internal: true,
    slug: "txt",
    label: "Plain Text (TXT)",
    extensions: ["txt", "text"],
  },
  {
    id: "csv",
    internal: true,
    slug: "csv",
    label: "Comma-Separated Values (CSV)",
    extensions: ["csv"],
  },
  {
    id: "eml",
    internal: true,
    slug: "eml",
    label: "Email Message (EML)",
    extensions: ["eml"],
  },
  {
    id: "msg",
    internal: true,
    slug: "msg",
    label: "Outlook Message (MSG)",
    extensions: ["msg"],
  },
  {
    id: "svg",
    internal: true,
    slug: "svg",
    label: "Scalable Vector Graphics (SVG)",
    extensions: ["svg"],
  },
  {
    id: "cbz",
    internal: true,
    slug: "cbz",
    label: "Comic Book Archive (CBZ)",
    extensions: ["cbz"],
  },
  {
    id: "odg",
    internal: true,
    slug: "odg",
    label: "OpenDocument Graphics (ODG)",
    extensions: ["odg"],
  },
  {
    id: "wpd",
    internal: true,
    slug: "wpd",
    label: "WordPerfect (WPD)",
    extensions: ["wpd"],
  },
  {
    id: "wps",
    internal: true,
    slug: "wps",
    label: "Microsoft Works (WPS)",
    extensions: ["wps"],
  },
  {
    id: "pub",
    internal: true,
    slug: "pub",
    label: "Microsoft Publisher (PUB)",
    extensions: ["pub"],
  },
  {
    id: "vsd",
    internal: true,
    slug: "vsd",
    label: "Microsoft Visio (VSD)",
    extensions: ["vsd"],
  },
  {
    id: "docling+json",
    internal: true,
    slug: "docling-json",
    label: "DoclingDocument (JSON)",
    extensions: [],
  },
  {
    id: "pandoc+json",
    internal: true,
    slug: "pandoc-json",
    label: "Pandoc AST (JSON)",
    extensions: [],
  },
];

const BY_ID = new Map(FORMATS.map((format) => [format.id, format]));
const BY_SLUG = new Map(FORMATS.map((format) => [format.slug, format]));

/** Catalogue key segment of a format: its slug with underscores, e.g. `ir_json`. */
export function formatMessageKey(format: FormatInfo): string {
  return format.slug.replaceAll("-", "_");
}

export function formatById(id: string): FormatInfo | undefined {
  return BY_ID.get(id);
}

/** A readable label for any registry id, including formats without display information. */
export function formatLabel(id: string): string {
  return BY_ID.get(id)?.label ?? id;
}

/** Desktop links use the short names accepted by native format selection. */
export function desktopLink(from: string, to: string, profile: string): string {
  const shortName = (id: string) =>
    id === "ariad-ir+json" ? "json" : id === "markdown" ? "md" : id;
  return `ariadshift://open?route=${encodeURIComponent(`${shortName(from)}-${shortName(to)}`)}&profile=${encodeURIComponent(profile)}`;
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

/** File picker `accept` value covering every active format with display information. */
export const ACCEPT = FORMATS.filter((format) => !format.internal)
  .flatMap((format) =>
    format.extensions.filter((extension) => !extension.includes(".")).map((ext) => `.${ext}`),
  )
  .join(",");
