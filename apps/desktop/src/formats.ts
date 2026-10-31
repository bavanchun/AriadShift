/** Output formats offered before the first file is inspected. */
export const formats = ["md", "html", "docx", "epub", "pdf", "json", "xlsx", "pptx"];

/** Host input formats and the file extensions the picker and drop target accept for each. */
export const inputExtensions: Record<string, string[]> = {
  md: ["md", "markdown", "mdown"],
  html: ["html", "htm"],
  docx: ["docx"],
  epub: ["epub"],
  pdf: ["pdf"],
  pptx: ["pptx"],
  xlsx: ["xlsx"],
  odt: ["odt"],
  ods: ["ods"],
  odp: ["odp"],
  doc: ["doc"],
  xls: ["xls"],
  ppt: ["ppt"],
  rtf: ["rtf"],
  json: ["json"],
  png: ["png"],
  jpeg: ["jpg", "jpeg"],
  tiff: ["tif", "tiff"],
  webp: ["webp"],
  "docling+json": ["json"],
};

const accepted = new Set(Object.values(inputExtensions).flat());

/** The final path component, for either path separator. */
export const fileName = (path: string) => path.split(/[\\/]/).at(-1) ?? path;

/** Whether the path names a file whose extension some input format accepts. */
export function isSupportedPath(path: string) {
  const base = fileName(path);
  const dot = base.lastIndexOf(".");
  return dot > 0 && accepted.has(base.slice(dot + 1).toLowerCase());
}

/** Splits dropped paths into supported documents and skipped items (folders, other types). */
export function partitionPaths(paths: string[]) {
  const supported: string[] = [];
  const skipped: string[] = [];
  for (const path of paths) (isSupportedPath(path) ? supported : skipped).push(path);
  return { supported, skipped };
}
