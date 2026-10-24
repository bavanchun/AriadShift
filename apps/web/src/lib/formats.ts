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
  {
    id: "pdf",
    slug: "pdf",
    label: "PDF",
    noun: "PDF document",
    extensions: ["pdf"],
    description:
      "Digital PDF text is read on this device. PDF output uses Typst and embedded fonts. Scanned pages need OCR in the desktop app.",
  },
  {
    id: "odt",
    slug: "odt",
    label: "OpenDocument Text (ODT)",
    noun: "OpenDocument text document",
    extensions: ["odt"],
    description: "OpenDocument text files use LibreOffice in the desktop app.",
  },
  {
    id: "rtf",
    slug: "rtf",
    label: "Rich Text (RTF)",
    noun: "Rich Text document",
    extensions: ["rtf"],
    description: "Rich Text documents use LibreOffice in the desktop app.",
  },
  {
    id: "pptx",
    slug: "pptx",
    label: "PowerPoint (PPTX)",
    noun: "PowerPoint presentation",
    extensions: ["pptx"],
    description: "PowerPoint presentations use Docling or LibreOffice in the desktop app.",
  },
  {
    id: "xlsx",
    slug: "xlsx",
    label: "Excel (XLSX)",
    noun: "Excel workbook",
    extensions: ["xlsx"],
    description: "Excel workbooks use Docling or LibreOffice in the desktop app.",
  },
  {
    id: "doc",
    slug: "doc",
    label: "Word (DOC)",
    noun: "legacy Word document",
    extensions: ["doc"],
    description: "Legacy Word documents use LibreOffice in the desktop app.",
  },
  {
    id: "xls",
    slug: "xls",
    label: "Excel (XLS)",
    noun: "legacy Excel workbook",
    extensions: ["xls"],
    description: "Legacy Excel workbooks use LibreOffice in the desktop app.",
  },
  {
    id: "ppt",
    slug: "ppt",
    label: "PowerPoint (PPT)",
    noun: "legacy PowerPoint presentation",
    extensions: ["ppt"],
    description: "Legacy PowerPoint presentations use LibreOffice in the desktop app.",
  },
  {
    id: "ods",
    slug: "ods",
    label: "OpenDocument Spreadsheet (ODS)",
    noun: "OpenDocument spreadsheet",
    extensions: ["ods"],
    description: "OpenDocument spreadsheets use LibreOffice in the desktop app.",
  },
  {
    id: "odp",
    slug: "odp",
    label: "OpenDocument Presentation (ODP)",
    noun: "OpenDocument presentation",
    extensions: ["odp"],
    description: "OpenDocument presentations use LibreOffice in the desktop app.",
  },
  {
    id: "png",
    slug: "png",
    label: "PNG",
    noun: "PNG image",
    extensions: ["png"],
    description: "Image text extraction needs the Docling engine pack in the desktop app.",
  },
  {
    id: "jpeg",
    slug: "jpeg",
    label: "JPEG",
    noun: "JPEG image",
    extensions: ["jpg", "jpeg"],
    description: "Image text extraction needs the Docling engine pack in the desktop app.",
  },
  {
    id: "webp",
    slug: "webp",
    label: "WebP",
    noun: "WebP image",
    extensions: ["webp"],
    description: "Image text extraction needs the Docling engine pack in the desktop app.",
  },
  {
    id: "tiff",
    slug: "tiff",
    label: "TIFF",
    noun: "TIFF image",
    extensions: ["tif", "tiff"],
    description: "Image text extraction needs the Docling engine pack in the desktop app.",
  },
  {
    id: "docling+json",
    slug: "docling-json",
    label: "DoclingDocument (JSON)",
    noun: "DoclingDocument JSON document",
    extensions: [],
    description: "Docling extraction results map to AriadShift IR on this device.",
  },
  {
    id: "pandoc+json",
    slug: "pandoc-json",
    label: "Pandoc AST (JSON)",
    noun: "Pandoc JSON document",
    extensions: [],
    description: "Internal interchange used by the Pandoc engine.",
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
