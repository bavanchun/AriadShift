// What the editor modes read from the active document: its pages, form fields, annotations and
// IR, plus the shared selection position and the export path from edited IR back to a file.

import { openOutput } from "@/converter/client";
import type { ExistingAnnotation, FormFieldInfo } from "@/editor/annotate";
import type { BlockPosition, IrDocument } from "@/editor/document/mapping";
import type { PageInfo } from "@/editor/page";
import type { WorkspaceClient } from "./types";

/** Where the user is in the document, shared by every editor mode. Pages are one-based. */
export interface EditorLocationState {
  page: number;
  /** Distance from the left edge of the page, in points. */
  xPt?: number;
  /** Distance from the top edge of the page, in points. */
  yPt?: number;
  blockId?: string;
}

/** Document mode's selection as a shared location; keeps the old one when nothing is selected. */
export function locationFromSelection(
  selection: { blockId: string | null; location: BlockPosition | null },
  previous: EditorLocationState,
): EditorLocationState {
  const { location, blockId } = selection;
  if (!location) return previous;
  return {
    page: location.page_no,
    xPt: location.bbox.left,
    yPt: location.bbox.top,
    ...(blockId ? { blockId } : {}),
  };
}

/** The point Document mode selects when it opens: the shared location, or a margin corner. */
export function initialPointFor(location: EditorLocationState) {
  return { pageNo: location.page, x: location.xPt ?? 72, y: location.yPt ?? 72 };
}

const record = (value: unknown): Record<string, unknown> =>
  typeof value === "object" && value !== null ? (value as Record<string, unknown>) : {};
const number = (value: unknown) =>
  typeof value === "number" && Number.isFinite(value) ? value : undefined;

const ANNOTATION_TYPES: ReadonlySet<string> = new Set([
  "highlight",
  "underline",
  "strikeout",
  "squiggly",
  "note",
  "free_text",
  "ink",
  "line",
  "arrow",
  "rectangle",
  "ellipse",
  "polygon",
  "polyline",
  "stamp",
  "link",
]);

const FIELD_TYPES: ReadonlySet<string> = new Set([
  "text",
  "checkbox",
  "radio",
  "combo",
  "list",
  "button",
  "signature",
]);

/** Page sizes from `pdf.info`. */
export function pagesFromInfo(data: unknown): PageInfo[] {
  const pages = record(data).pages;
  if (!Array.isArray(pages)) return [];
  return pages.flatMap((entry) => {
    const item = record(entry);
    const page = number(item.page);
    const widthPt = number(item.width_pt);
    const heightPt = number(item.height_pt);
    return page !== undefined && widthPt && heightPt ? [{ page, widthPt, heightPt }] : [];
  });
}

/** Form fields from `pdf.forms_list`. */
export function fieldsFromList(data: unknown): FormFieldInfo[] {
  const fields = record(data).fields;
  if (!Array.isArray(fields)) return [];
  return fields.flatMap((entry) => {
    const field = record(entry);
    const rect = Array.isArray(field.rect) ? field.rect.map(number) : [];
    if (
      typeof field.name !== "string" ||
      typeof field.type !== "string" ||
      !FIELD_TYPES.has(field.type) ||
      number(field.page) === undefined ||
      rect.length !== 4 ||
      rect.some((value) => value === undefined)
    )
      return [];
    return [field as unknown as FormFieldInfo];
  });
}

/** Annotations from `pdf.annotations` with `action: "list"`. */
export function annotationsFromList(data: unknown): ExistingAnnotation[] {
  const annotations = record(data).annotations;
  if (!Array.isArray(annotations)) return [];
  return annotations.flatMap((entry) => {
    const item = record(entry);
    const page = number(item.page);
    if (typeof item.id !== "string" || page === undefined) return [];
    if (typeof item.type !== "string" || !ANNOTATION_TYPES.has(item.type)) return [];
    const rect = Array.isArray(item.rect) ? item.rect.map(number) : [];
    return [
      {
        id: item.id,
        page,
        type: item.type as ExistingAnnotation["type"],
        ...(rect.length === 4 && rect.every((value) => value !== undefined)
          ? { rect: rect as unknown as NonNullable<ExistingAnnotation["rect"]> }
          : {}),
      },
    ];
  });
}

export interface ActiveDocumentData {
  pages: PageInfo[];
  fields: FormFieldInfo[];
  annotations: ExistingAnnotation[];
}

/**
 * Reads the active document's pages, fields and annotations. Without `pdf.info` there is no page
 * to draw, so that failure is thrown; a form or annotation list that fails reads as empty.
 */
export async function readDocumentData(
  client: WorkspaceClient,
  inputId: string,
  budgetBytes?: number,
): Promise<ActiveDocumentData> {
  const call = async (toolId: string, params: Record<string, unknown>) =>
    (
      await client.runTool(toolId, params, {
        inputIds: [inputId],
        ...(budgetBytes === undefined ? {} : { budgetBytes }),
      })
    ).data;
  const optional = (toolId: string, params: Record<string, unknown>) =>
    call(toolId, params).catch(() => null);
  const pages = pagesFromInfo(await call("pdf.info", {}));
  const fields = fieldsFromList(await optional("pdf.forms_list", {}));
  const annotations = annotationsFromList(await optional("pdf.annotations", { action: "list" }));
  return { pages, fields, annotations };
}

/** Converts the active PDF to `ariad-ir/2` JSON and parses it. */
export async function loadIr(client: WorkspaceClient, inputId: string): Promise<IrDocument> {
  const converted = await client.convert("ariad-ir+json", "editable", { inputId });
  const text = await (await openOutput(converted.location)).text();
  const ir = JSON.parse(text) as IrDocument;
  if (ir.version !== "ariad-ir/2") throw new Error("The converter returned an unsupported IR.");
  return ir;
}

export type EditorExportFormat = "pdf" | "docx" | "md" | "html";

const EXPORT_TARGET: Record<EditorExportFormat, { to: string; mediaType: string }> = {
  pdf: { to: "pdf", mediaType: "application/pdf" },
  docx: {
    to: "docx",
    mediaType: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
  },
  md: { to: "markdown", mediaType: "text/markdown" },
  html: { to: "html", mediaType: "text/html" },
};

export interface EditorExport {
  name: string;
  size: number;
  format: EditorExportFormat;
  mediaType: string;
  blob: Blob;
}

/** Converts the edited IR to a file through the browser converter. */
export async function exportEditedIr(
  client: WorkspaceClient,
  irJson: string,
  format: EditorExportFormat,
  stem: string,
): Promise<EditorExport> {
  const target = EXPORT_TARGET[format];
  const file = new File([irJson], `${stem}.ir.json`, { type: "application/json" });
  const converted = await client.convert(target.to, "editable", { file });
  const blob = await openOutput(converted.location);
  return {
    name: converted.name,
    size: converted.size,
    format,
    mediaType: converted.mediaType || target.mediaType,
    blob,
  };
}
