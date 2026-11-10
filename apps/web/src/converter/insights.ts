import type { Core } from "./engines";
import type { ToolDescriptor } from "./tool-catalog";

export interface PdfFileInsights {
  pageCount: number;
  scannedPageCount: number;
  scannedPages: number[];
  blankPageCount: number;
  blankPages: number[];
  /** Estimate assumes each image covers its page; it is not a measured placement DPI. */
  estimatedImageDpi: { min: number | null; max: number | null; lowDpiPages: number[] };
  emptyRequiredFormFields: { count: number; names: string[] };
  signatureFieldCount: number;
  hasMetadata: boolean;
  metadataKeys: string[];
  hasEditHistory: boolean;
  encrypted: boolean;
  linearized: boolean;
  hasForms: boolean;
  hasAnnotations: boolean;
  hasAttachments: boolean;
  hasJavascript: boolean;
  outlineCount: number;
  suggestedTools: string[];
  warnings: string[];
  recycle: boolean;
}

type RecordValue = Record<string, unknown>;
const record = (value: unknown): RecordValue =>
  value !== null && typeof value === "object" ? (value as RecordValue) : {};

/** Reads dictionary prefixes of ordinary indirect objects, without decoding streams in JavaScript. */
function dictionaries(text: string): Map<string, string> {
  const objects = new Map<string, string>();
  for (const match of text.matchAll(/\b(\d+)\s+(\d+)\s+obj\b([\s\S]*?)\bendobj\b/g)) {
    objects.set(`${match[1]} ${match[2]}`, (match[3] ?? "").split(/\bstream\r?\n/, 1)[0] ?? "");
  }
  return objects;
}

const references = (text: string) =>
  [...text.matchAll(/\b(\d+)\s+(\d+)\s+R\b/g)].map((match) => `${match[1]} ${match[2]}`);

function imageDpi(
  objects: Map<string, string>,
  pages: RecordValue[],
): PdfFileInsights["estimatedImageDpi"] {
  const catalog = [...objects.values()].find((dict) => /\/Type\s*\/Catalog\b/.test(dict));
  const root = catalog?.match(/\/Pages\s+(\d+\s+\d+\s+R)/)?.[1];
  const ordered: string[] = [];
  const visited = new Set<string>();
  function walk(id: string) {
    if (visited.has(id)) return;
    visited.add(id);
    const dict = objects.get(id) ?? "";
    if (/\/Type\s*\/Page\b/.test(dict)) ordered.push(id);
    else for (const kid of references(dict.match(/\/Kids\s*\[([^\]]*)\]/)?.[1] ?? "")) walk(kid);
  }
  if (root) for (const id of references(root)) walk(id);
  // No reliable page association means no invented DPI or low-resolution page number.
  if (ordered.length !== pages.length) return { min: null, max: null, lowDpiPages: [] };
  const dpi: number[] = [];
  const lowDpiPages: number[] = [];
  for (const [index, id] of ordered.entries()) {
    let dict = objects.get(id) ?? "";
    const ancestors = new Set<string>();
    while (!/\/Resources\b/.test(dict)) {
      const parent = references(dict.match(/\/Parent\s+(\d+\s+\d+\s+R)/)?.[1] ?? "")[0];
      if (!parent || ancestors.has(parent)) break;
      ancestors.add(parent);
      dict = objects.get(parent) ?? "";
    }
    const seen = new Set<string>();
    const page = pages[index] ?? {};
    const width = Number(page.width_pt);
    const height = Number(page.height_pt);
    if (!(width > 0 && height > 0)) continue;
    const pageDpi: number[] = [];
    const pending = [dict];
    while (pending.length) {
      const body = pending.pop() as string;
      if (/\/Subtype\s*\/Image\b/.test(body)) {
        const w = Number(body.match(/\/Width\s+(\d+)/)?.[1]);
        const h = Number(body.match(/\/Height\s+(\d+)/)?.[1]);
        if (w > 0 && h > 0) pageDpi.push((w * 72) / width, (h * 72) / height);
        continue;
      }
      // Follow only resources and their XObjects, excluding page-tree back references.
      const resources = body.includes("/Resources") ? body.slice(body.indexOf("/Resources")) : body;
      const bounded = resources.replace(
        /\/(?:Parent|Contents|Kids|Pages)\s+(?:\d+\s+\d+\s+R|\[[^\]]*\])/g,
        "",
      );
      for (const ref of references(bounded)) {
        if (seen.has(ref)) continue;
        seen.add(ref);
        pending.push(objects.get(ref) ?? "");
      }
    }
    for (const value of pageDpi) dpi.push(value);
    if (pageDpi.some((value) => value < 150)) lowDpiPages.push(index + 1);
  }
  return {
    min: dpi.length ? Math.round(dpi.reduce((min, value) => Math.min(min, value), Infinity)) : null,
    max: dpi.length ? Math.round(dpi.reduce((max, value) => Math.max(max, value), 0)) : null,
    lowDpiPages,
  };
}

/** All analysis stays inside the worker; no network or persistent parameter storage. */
export function inspectPdfInsights(
  core: Core,
  bytes: Uint8Array,
  name: string,
  budgetBytes: number,
  catalog: ToolDescriptor[],
): PdfFileInsights {
  const runTool = core.runTool;
  if (!runTool)
    throw { code: "engine_unavailable", message: "The browser tool engine is unavailable." };
  const run = (id: string) => runTool([bytes], [name], id, "{}", budgetBytes, false);
  const infoResult = run("pdf.info");
  const info = record(infoResult.data);
  const warnings = [...infoResult.warnings.map((warning) => warning.message)];
  const pages = Array.isArray(info.pages) ? info.pages.map(record) : [];
  const scannedPages = pages.flatMap((page, index) =>
    page.image_only === true || page.has_text_layer === false
      ? [Number(page.page ?? index + 1)]
      : [],
  );
  const blankPages = pages.filter((page) => page.blank === true).map((page) => Number(page.page));
  let fields: RecordValue[] = [];
  if (info.has_forms === true) {
    const formTool =
      catalog.find((tool) => tool.id === "form.schema") ??
      catalog.find((tool) => tool.id === "pdf.forms_list");
    if (formTool) {
      const result = run(formTool.id);
      const data = record(result.data);
      fields = Array.isArray(data.fields) ? data.fields.map(record) : [];
      warnings.push(...result.warnings.map((warning) => warning.message));
      if (data.truncated === true) warnings.push("The form field listing was truncated.");
    }
  }
  const empty = fields
    .filter(
      (field) =>
        (field.required === true || record(field.flags).required === true) &&
        (field.value == null ||
          field.value === false ||
          field.value === "" ||
          field.value === "Off" ||
          (Array.isArray(field.value) && field.value.length === 0)),
    )
    .map((field) => String(field.name));
  const original = new TextDecoder("latin1").decode(bytes);
  let text = original;
  if (/\/Type\s*\/ObjStm\b/.test(text) && info.encrypted !== true) {
    // Expands compressed object dictionaries through the existing structural engine.
    text = new TextDecoder("latin1").decode(run("pdf.decompress").bytes);
  }
  const objects = dictionaries(text);
  const dictionaryText = [...objects.values()].join("\n");
  const signatures = fields.filter(
    (field) => field.field_type === "signature" || field.type === "signature",
  ).length;
  const signatureFieldCount =
    signatures ||
    Math.max(
      [...dictionaryText.matchAll(/\/FT\s*\/Sig\b/g)].length,
      [...dictionaryText.matchAll(/\/Type\s*\/Sig\b/g)].length,
    );
  const metadata = record(info.metadata);
  const keys = [
    "title",
    "author",
    "creator",
    "producer",
    "creation_date",
    "mod_date",
    "subject",
    "keywords",
  ];
  const metadataKeys = keys.filter(
    (key) => typeof metadata[key] === "string" && metadata[key] !== "",
  );
  // pdf.info currently omits Subject/Keywords; consult only the trailer's Info dictionary.
  const infoRef = [...original.matchAll(/\/Info\s+(\d+\s+\d+\s+R)/g)].at(-1)?.[1];
  const infoDict = objects.get(references(infoRef ?? "")[0] ?? "") ?? "";
  for (const [key, pdfKey] of [
    ["subject", "Subject"],
    ["keywords", "Keywords"],
  ]) {
    if (
      new RegExp(`/${pdfKey}\\s*(?:\\((?:\\\\.|[^)\\\\])+\\)|<[0-9a-fA-F]+>)`).test(infoDict) &&
      !metadataKeys.includes(key as string)
    )
      metadataKeys.push(key as string);
  }
  const hasEditHistory =
    [...original.matchAll(/^\s*%%EOF\s*$/gm)].length > 1 ||
    (typeof metadata.mod_date === "string" &&
      typeof metadata.creation_date === "string" &&
      metadata.mod_date !== metadata.creation_date);
  const suggestedTools: string[] = [];
  const suggest = (...ids: string[]) => {
    const tool = ids.find((id) => catalog.some((candidate) => candidate.id === id));
    if (tool && !suggestedTools.includes(tool)) suggestedTools.push(tool);
  };
  if (scannedPages.length) suggest("ocr.pdf", "pdf.ocr");
  if (blankPages.length) suggest("pdf.delete_pages");
  if (empty.length) suggest("form.fill", "pdf.forms_fill");
  if (signatureFieldCount) suggest("sign.verify", "pdf.sign_verify");
  if (metadataKeys.length || hasEditHistory || info.has_javascript === true)
    suggest("pdf.sanitize", "pdf.scrub_metadata", "pdf.metadata");
  const estimatedImageDpi = imageDpi(objects, pages);
  if (pages.some((page) => page.image_only === true) && estimatedImageDpi.min === null) {
    warnings.push("Image DPI could not be estimated from the PDF page resources.");
  }
  return {
    pageCount: Number(info.page_count ?? pages.length),
    scannedPageCount: scannedPages.length,
    scannedPages,
    blankPageCount: blankPages.length,
    blankPages,
    estimatedImageDpi,
    emptyRequiredFormFields: { count: empty.length, names: empty },
    signatureFieldCount,
    hasMetadata: metadataKeys.length > 0,
    metadataKeys,
    hasEditHistory,
    encrypted: info.encrypted === true,
    linearized: info.linearized === true,
    hasForms: info.has_forms === true,
    hasAnnotations: info.has_annotations === true,
    hasAttachments: info.has_attachments === true,
    hasJavascript: info.has_javascript === true,
    outlineCount: Number(info.outline_count ?? 0),
    suggestedTools,
    warnings,
    recycle: core.memoryBytes() > 256 * 1024 * 1024,
  };
}
