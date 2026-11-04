/** Output formats offered before the first file is inspected. */
export const formats = ["md", "html", "docx", "epub", "pdf", "json", "xlsx", "pptx"];

/** All user-facing format extensions known to AriadShift. */
export const allFormats: Record<string, string[]> = {
  md: ["md", "markdown", "mdown"],
  html: ["html", "htm"],
  docx: ["docx"],
  epub: ["epub"],
  pdf: ["pdf"],
  png: ["png"],
  json: ["json"],
  pptx: ["pptx"],
  xlsx: ["xlsx"],
  odt: ["odt"],
  ods: ["ods"],
  odp: ["odp"],
  doc: ["doc"],
  xls: ["xls"],
  ppt: ["ppt"],
  rtf: ["rtf"],
  jpeg: ["jpg", "jpeg"],
  webp: ["webp"],
  tiff: ["tif", "tiff"],
  txt: ["txt", "text"],
  csv: ["csv"],
  eml: ["eml"],
  msg: ["msg"],
  svg: ["svg"],
  cbz: ["cbz"],
  odg: ["odg"],
  wpd: ["wpd"],
  wps: ["wps"],
  pub: ["pub"],
  vsd: ["vsd"],
};

/** Formats that currently have active conversion routes. */
const routableFormats = new Set([
  "md",
  "html",
  "docx",
  "epub",
  "pdf",
  "pptx",
  "xlsx",
  "odt",
  "ods",
  "odp",
  "doc",
  "xls",
  "ppt",
  "rtf",
  "json",
  "png",
  "jpeg",
  "tiff",
  "webp",
]);

/** Host input formats and the file extensions the picker and drop target accept for each.
 * Only formats with active engine conversion routes are accepted as inputs.
 */
export const inputExtensions: Record<string, string[]> = {
  ...Object.fromEntries(
    Object.entries(allFormats).filter(([format]) => routableFormats.has(format)),
  ),
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
