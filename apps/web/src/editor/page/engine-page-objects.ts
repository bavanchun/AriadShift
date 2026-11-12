// Page mode input: the objects of a PDF as `PageObject[]`. They come from the engine's
// `edit.page_objects` report (`ariad-pdf-page-objects/1`) when a runtime can produce it, and
// otherwise from the IR layout of the same PDF, one text object per positioned block.
import type { BlockPosition, IrBlock, IrDocument, IrInline } from "@/editor/document/mapping";
import type { ImageObject, PageBBox, PageInfo, PageObject, TextRunObject } from "./page-model";

export interface PageObjectsModel {
  pages: PageInfo[];
  objects: PageObject[];
  /** `engine`: reported by `edit.page_objects`. `ir`: estimated from the IR layout. */
  source: "engine" | "ir";
}

const SCHEMA = "ariad-pdf-page-objects/1";
const SUBSET_PREFIX = /^[A-Z]{6}\+/;
const DEFAULT_FONT = "Helvetica";
const DEFAULT_SIZE_PT = 12;

type Raw = Record<string, unknown>;
const isRecord = (value: unknown): value is Raw => typeof value === "object" && value !== null;
const num = (value: unknown, fallback: number) =>
  typeof value === "number" && Number.isFinite(value) ? value : fallback;
const str = (value: unknown): string | undefined =>
  typeof value === "string" && value.length > 0 ? value : undefined;

/** `#RRGGBBAA` and `#RGB` become `#RRGGBB`; anything else becomes the fallback. */
export function normalizeColor(value: unknown, fallback: string): string {
  const text = str(value);
  if (!text) return fallback;
  const full = /^#([0-9a-f]{6})(?:[0-9a-f]{2})?$/i.exec(text);
  if (full) return `#${(full[1] as string).toUpperCase()}`;
  const short = /^#([0-9a-f]{3})$/i.exec(text);
  if (short) {
    return `#${[...(short[1] as string)].map((char) => char + char).join("")}`.toUpperCase();
  }
  return fallback;
}

/**
 * A font program only carries the glyphs its document used. The engine does not list them, so
 * the glyph set of each font is the characters its runs already show. A subset font therefore
 * flags exactly the characters the document never drew, such as Vietnamese tone marks.
 */
export function withEmbeddedGlyphs(objects: readonly PageObject[]): PageObject[] {
  const glyphs = new Map<string, Set<string>>();
  for (const object of objects) {
    if (object.kind !== "text") continue;
    const set = glyphs.get(object.fontName) ?? new Set<string>();
    for (const char of object.text.normalize("NFC")) set.add(char);
    glyphs.set(object.fontName, set);
  }
  return objects.map((object) =>
    object.kind === "text" && object.isSubset
      ? { ...object, embeddedGlyphs: [...(glyphs.get(object.fontName) ?? [])].join("") }
      : object,
  );
}

function bbox(value: unknown): PageBBox {
  const list = Array.isArray(value) ? value : [];
  return [num(list[0], 0), num(list[1], 0), num(list[2], 0), num(list[3], 0)];
}

/** Maps a parsed `edit.page_objects` document; throws when it is not that document. */
export function mapEnginePageObjects(raw: unknown): PageObjectsModel {
  if (!isRecord(raw) || raw.schema !== SCHEMA || !Array.isArray(raw.pages)) {
    throw new Error(`Expected an ${SCHEMA} document.`);
  }
  const pages: PageInfo[] = [];
  const objects: PageObject[] = [];
  for (const entry of raw.pages) {
    if (!isRecord(entry)) continue;
    const page = num(entry.page, 0);
    if (page < 1) continue;
    pages.push({ page, widthPt: num(entry.width_pt, 0), heightPt: num(entry.height_pt, 0) });
    for (const item of Array.isArray(entry.objects) ? entry.objects : []) {
      if (!isRecord(item)) continue;
      const id = str(item.id);
      if (!id) continue;
      const box = bbox(item.bbox);
      if (item.kind === "text") {
        const fontName = str(item.font_name) ?? DEFAULT_FONT;
        objects.push({
          kind: "text",
          id,
          page,
          bbox: box,
          text: typeof item.text === "string" ? item.text : "",
          fontName,
          fontSize: num(item.font_size, DEFAULT_SIZE_PT),
          color: normalizeColor(item.fill_color, "#000000"),
          isSubset: SUBSET_PREFIX.test(fontName),
        });
      } else if (item.kind === "image") {
        objects.push({
          kind: "image",
          id,
          page,
          bbox: box,
          width: num(item.width_px, 0),
          height: num(item.height_px, 0),
        });
      } else if (item.kind === "path") {
        objects.push({
          kind: "path",
          id,
          page,
          bbox: box,
          strokeColor: normalizeColor(item.stroke_color, "#000000"),
          fillColor: normalizeColor(item.fill_color, "none"),
        });
      }
    }
  }
  return { pages, objects: withEmbeddedGlyphs(objects), source: "engine" };
}

function inlineText(inlines: readonly IrInline[] | undefined): string {
  let text = "";
  for (const inline of inlines ?? []) {
    if (typeof inline.text === "string") text += inline.text;
    else if (inline.type === "line_break" || inline.type === "soft_break") text += " ";
    else text += inlineText(inline.content);
  }
  return text;
}

interface FirstRun {
  family?: string;
  size?: number;
  color?: string;
}

function firstRun(inlines: readonly IrInline[] | undefined, found: FirstRun = {}): FirstRun {
  for (const inline of inlines ?? []) {
    const direct = inline.direct;
    if (direct?.font?.family && !found.family) found.family = direct.font.family;
    if (direct?.size_pt && !found.size) found.size = direct.size_pt;
    if (direct?.color && !found.color) found.color = direct.color;
    firstRun(inline.content, found);
  }
  return found;
}

/** Positioned blocks in reading order, descending into containers that have no position. */
function* positioned(
  blocks: readonly IrBlock[],
  layout: Record<string, BlockPosition>,
): Generator<{ block: IrBlock; position: BlockPosition }> {
  for (const block of blocks) {
    const position = layout[block.id];
    if (position) yield { block, position };
    else {
      yield* positioned(block.blocks ?? [], layout);
      for (const item of block.items ?? []) yield* positioned(item.blocks, layout);
    }
  }
}

/**
 * Estimates the page objects of a PDF from its IR layout, for runtimes that cannot run
 * `edit.page_objects`. Text blocks become text objects (a block's first character style gives
 * the font, size and colour), figures become image objects. The ids are IR block ids, so the
 * edits made on them describe a document the engine can only apply after inspecting it itself.
 */
export function pageObjectsFromIr(ir: IrDocument): PageObjectsModel {
  const layout = ir.layout?.blocks ?? {};
  const pages: PageInfo[] = (ir.layout?.pages ?? []).map((page) => ({
    page: page.page_no,
    widthPt: page.width,
    heightPt: page.height,
  }));
  const height = new Map(pages.map((page) => [page.page, page.heightPt]));
  const objects: PageObject[] = [];
  for (const { block, position } of positioned(ir.body, layout)) {
    const pageHeight = height.get(position.page_no);
    if (pageHeight === undefined) continue;
    const { left, top, right, bottom } = position.bbox;
    const box: PageBBox = [left, pageHeight - bottom, right, pageHeight - top];
    const id = `ir:${block.id}`;
    if (block.asset) {
      const image: ImageObject = {
        kind: "image",
        id,
        page: position.page_no,
        bbox: box,
        width: 0,
        height: 0,
      };
      objects.push(image);
      continue;
    }
    const text = inlineText(block.content).trim();
    if (text.length === 0) continue;
    const run = firstRun(block.content);
    const font = run.family ?? block.direct?.font?.family ?? DEFAULT_FONT;
    const object: TextRunObject = {
      kind: "text",
      id,
      page: position.page_no,
      bbox: box,
      text,
      fontName: font,
      fontSize: run.size ?? block.direct?.size_pt ?? DEFAULT_SIZE_PT,
      color: normalizeColor(run.color ?? block.direct?.color, "#000000"),
      // A PDF font is embedded as a subset unless proven otherwise.
      isSubset: true,
    };
    objects.push(object);
  }
  return { pages, objects: withEmbeddedGlyphs(objects), source: "ir" };
}
