// The Page mode edit session: applies in-place edits to a snapshot of the page objects, keeps the
// undo/redo history, and serialises the net result into the `edit.apply` tool parameters.
import {
  type FontResolution,
  type ImageObject,
  type PageBBox,
  type PageObject,
  resolveFontForEdit,
  type TextRunObject,
} from "./page-model";

/** `edit.apply` operations (`PageEditOp` in `ariad-core`), unrotated PDF points. */
export type PageEditOp =
  | { op: "replace_text"; page: number; object_id: string; text: string }
  | {
      op: "set_text_style";
      page: number;
      object_id: string;
      font_name?: string;
      font_size?: number;
      color?: string;
    }
  | { op: "move_object"; page: number; object_id: string; dx_pt: number; dy_pt: number }
  | { op: "delete_object"; page: number; object_id: string }
  | {
      op: "add_text";
      page: number;
      text: string;
      x_pt: number;
      y_pt: number;
      font_name?: string;
      font_size: number;
      color?: string;
    }
  | { op: "add_image"; page: number; bbox: [number, number, number, number]; image_base64: string };

export interface EditApplyParams {
  ops: PageEditOp[];
  save: "full" | "incremental";
}

export interface TextStylePatch {
  fontName?: string;
  fontSize?: number;
  color?: string;
}

export interface NewTextBox {
  page: number;
  x: number;
  y: number;
  text: string;
  fontSize?: number;
  color?: string;
  fontName?: string;
}

export interface NewImage {
  page: number;
  bbox: PageBBox;
  dataUrl: string;
  width: number;
  height: number;
}

export const MAX_EDIT_OPS = 1000;
const MIN_OBJECT_PT = 1;
const HEX_COLOR = /^#[0-9A-Fa-f]{6}$/;
const EPSILON = 1e-6;

const round = (value: number) => Math.round(value * 1000) / 1000;
const finite = (...values: number[]) => values.every(Number.isFinite);
const width = (bbox: PageBBox) => bbox[2] - bbox[0];
const height = (bbox: PageBBox) => bbox[3] - bbox[1];

function validSize(size: number): boolean {
  return Number.isFinite(size) && size > 0 && size <= 1000;
}

/** Existing images can only be resized when their pixels are known, so they can be re-placed. */
export function canResize(object: PageObject): boolean {
  if (object.kind === "text") return true;
  return object.kind === "image" && (object.dataUrl !== undefined || isNewObject(object.id));
}

const NEW_ID = /^new\d+$/;
export const isNewObject = (id: string) => NEW_ID.test(id);

function base64Payload(dataUrl: string): string {
  const comma = dataUrl.indexOf(",");
  return dataUrl.startsWith("data:") && comma >= 0 ? dataUrl.slice(comma + 1) : dataUrl;
}

export class PageEditSession {
  readonly #base: readonly PageObject[];
  readonly #baseById: ReadonlyMap<string, PageObject>;
  #objects: readonly PageObject[];
  #past: (readonly PageObject[])[] = [];
  #future: (readonly PageObject[])[] = [];
  #nextId = 1;
  readonly #listeners = new Set<() => void>();

  constructor(objects: readonly PageObject[]) {
    this.#base = objects;
    this.#baseById = new Map(objects.map((object) => [object.id, object]));
    this.#objects = objects;
  }

  /** The current objects; the array identity changes with every committed edit. */
  get objects(): readonly PageObject[] {
    return this.#objects;
  }
  get canUndo(): boolean {
    return this.#past.length > 0;
  }
  get canRedo(): boolean {
    return this.#future.length > 0;
  }
  get hasChanges(): boolean {
    return this.#objects !== this.#base;
  }

  /** For `useSyncExternalStore`. */
  readonly subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  };
  readonly getSnapshot = (): readonly PageObject[] => this.#objects;

  get(id: string): PageObject | undefined {
    return this.#objects.find((object) => object.id === id);
  }
  original(id: string): PageObject | undefined {
    return this.#baseById.get(id);
  }
  onPage(page: number): PageObject[] {
    return this.#objects.filter((object) => object.page === page);
  }
  /** The object differs from the source document (edited, moved, restyled or newly added). */
  isModified(id: string): boolean {
    const current = this.get(id);
    return current !== undefined && current !== this.#baseById.get(id);
  }

  editText(id: string, text: string): boolean {
    const object = this.get(id);
    if (object?.kind !== "text") return false;
    const next = text.normalize("NFC");
    if (next.length === 0 || next === object.text) return false;
    return this.#replace({ ...object, text: next });
  }

  editTextStyle(id: string, patch: TextStylePatch): boolean {
    const object = this.get(id);
    if (object?.kind !== "text") return false;
    const next: TextRunObject = { ...object };
    if (patch.fontName !== undefined && patch.fontName.length > 0) next.fontName = patch.fontName;
    if (patch.fontSize !== undefined && validSize(patch.fontSize)) next.fontSize = patch.fontSize;
    if (patch.color !== undefined && HEX_COLOR.test(patch.color)) next.color = patch.color;
    const same =
      next.fontName === object.fontName &&
      next.fontSize === object.fontSize &&
      next.color === object.color;
    return same ? false : this.#replace(next);
  }

  moveObject(id: string, dx: number, dy: number): boolean {
    const object = this.get(id);
    if (!object || !finite(dx, dy) || (dx === 0 && dy === 0)) return false;
    const [left, bottom, right, top] = object.bbox;
    return this.#replace({ ...object, bbox: [left + dx, bottom + dy, right + dx, top + dy] });
  }

  /**
   * Text scales uniformly (the engine can only change a run's font size), images resize freely,
   * and paths and source images without pixel data cannot be resized.
   */
  resizeObject(id: string, bbox: PageBBox): boolean {
    const object = this.get(id);
    if (!object || !canResize(object) || !finite(...bbox)) return false;
    if (width(bbox) < MIN_OBJECT_PT || height(bbox) < MIN_OBJECT_PT) return false;
    if (object.kind === "text") {
      const heightChanged = Math.abs(height(bbox) - height(object.bbox)) > EPSILON;
      const scale = heightChanged
        ? height(bbox) / height(object.bbox)
        : width(bbox) / width(object.bbox);
      if (Math.abs(scale - 1) < EPSILON) return false;
      const fontSize = Math.round(object.fontSize * scale * 10) / 10;
      if (!validSize(fontSize)) return false;
      const [left, bottom] = object.bbox;
      return this.#replace({
        ...object,
        fontSize,
        bbox: [
          left,
          bottom,
          left + width(object.bbox) * scale,
          bottom + height(object.bbox) * scale,
        ],
      });
    }
    return this.#replace({ ...object, bbox: [...bbox] });
  }

  deleteObject(id: string): boolean {
    if (!this.get(id)) return false;
    return this.#commit(this.#objects.filter((object) => object.id !== id));
  }

  /** Adds a text box whose baseline origin is `x`, `y`; returns the new object's id. */
  addTextBox(box: NewTextBox): string | undefined {
    const text = box.text.normalize("NFC");
    const fontSize = box.fontSize ?? 12;
    if (text.length === 0 || !validSize(fontSize) || !finite(box.x, box.y)) return undefined;
    const id = this.#newId();
    const object: TextRunObject = {
      kind: "text",
      id,
      page: box.page,
      bbox: [
        box.x,
        box.y,
        box.x + Math.max(text.length, 1) * fontSize * 0.6,
        box.y + fontSize * 1.2,
      ],
      text,
      fontName: box.fontName ?? "Noto Sans",
      fontSize,
      color: box.color && HEX_COLOR.test(box.color) ? box.color : "#000000",
    };
    return this.#commit([...this.#objects, object]) ? id : undefined;
  }

  addImage(image: NewImage): string | undefined {
    const [left, bottom, right, top] = image.bbox;
    if (!finite(left, bottom, right, top) || right <= left || top <= bottom || !image.dataUrl) {
      return undefined;
    }
    const id = this.#newId();
    const object: ImageObject = {
      kind: "image",
      id,
      page: image.page,
      bbox: image.bbox,
      width: image.width,
      height: image.height,
      dataUrl: image.dataUrl,
    };
    return this.#commit([...this.#objects, object]) ? id : undefined;
  }

  undo(): boolean {
    const previous = this.#past.pop();
    if (!previous) return false;
    this.#future.push(this.#objects);
    this.#objects = previous;
    this.#emit();
    return true;
  }

  redo(): boolean {
    const next = this.#future.pop();
    if (!next) return false;
    this.#past.push(this.#objects);
    this.#objects = next;
    this.#emit();
    return true;
  }

  /** Discards every edit and the history, returning to the source document. */
  resetChanges(): void {
    if (!this.hasChanges && !this.canUndo && !this.canRedo) return;
    this.#past = [];
    this.#future = [];
    this.#objects = this.#base;
    this.#emit();
  }

  /** The font substitution an edited run would trigger, if any. */
  substitutionFor(id: string): FontResolution | undefined {
    const original = this.#baseById.get(id);
    const current = this.get(id);
    if (original?.kind !== "text" || current?.kind !== "text") return undefined;
    // An explicit family choice overrides the automatic fallback chain.
    if (current.text === original.text || current.fontName !== original.fontName) return undefined;
    const resolution = resolveFontForEdit(original, current.text);
    return resolution.substituted ? resolution : undefined;
  }

  substitutions(): { id: string; resolution: FontResolution }[] {
    const found: { id: string; resolution: FontResolution }[] = [];
    for (const object of this.#objects) {
      const resolution = this.substitutionFor(object.id);
      if (resolution) found.push({ id: object.id, resolution });
    }
    return found;
  }

  /** The net difference from the source document, as ordered `edit.apply` operations. */
  toEditPageParams(save: EditApplyParams["save"] = "full"): EditApplyParams {
    const ops: PageEditOp[] = [];
    const additions: PageEditOp[] = [];
    const current = new Map(this.#objects.map((object) => [object.id, object]));

    for (const original of this.#base) {
      const next = current.get(original.id);
      if (!next) {
        ops.push({ op: "delete_object", page: original.page, object_id: original.id });
      } else if (next !== original) {
        this.#diff(original, next, ops, additions);
      }
    }
    for (const object of this.#objects) {
      if (!this.#baseById.has(object.id)) additions.push(this.#addition(object));
    }
    return { ops: [...ops, ...additions], save };
  }

  #diff(original: PageObject, next: PageObject, ops: PageEditOp[], additions: PageEditOp[]) {
    const { page, id } = original;
    if (original.kind === "text" && next.kind === "text") {
      if (next.text !== original.text) {
        ops.push({ op: "replace_text", page, object_id: id, text: next.text });
      }
      const style: Extract<PageEditOp, { op: "set_text_style" }> = {
        op: "set_text_style",
        page,
        object_id: id,
      };
      if (next.fontName !== original.fontName) style.font_name = next.fontName;
      if (next.fontSize !== original.fontSize) style.font_size = next.fontSize;
      if (next.color !== original.color) style.color = next.color;
      if (Object.keys(style).length > 3) ops.push(style);
    }
    const resized =
      Math.abs(width(next.bbox) - width(original.bbox)) > EPSILON ||
      Math.abs(height(next.bbox) - height(original.bbox)) > EPSILON;
    if (original.kind === "image" && next.kind === "image" && resized && next.dataUrl) {
      // The engine cannot resize an image, so the resized copy replaces the original.
      ops.push({ op: "delete_object", page, object_id: id });
      additions.push(this.#addition(next));
      return;
    }
    const dx = round(next.bbox[0] - original.bbox[0]);
    const dy = round(next.bbox[1] - original.bbox[1]);
    if (dx !== 0 || dy !== 0) {
      ops.push({ op: "move_object", page, object_id: id, dx_pt: dx, dy_pt: dy });
    }
  }

  #addition(object: PageObject): PageEditOp {
    if (object.kind === "text") {
      return {
        op: "add_text",
        page: object.page,
        text: object.text,
        x_pt: round(object.bbox[0]),
        y_pt: round(object.bbox[1]),
        font_size: object.fontSize,
        color: object.color,
      };
    }
    const [left, bottom, right, top] = object.bbox;
    return {
      op: "add_image",
      page: object.page,
      bbox: [round(left), round(bottom), round(right), round(top)],
      image_base64: base64Payload(object.kind === "image" ? (object.dataUrl ?? "") : ""),
    };
  }

  #newId(): string {
    return `new${this.#nextId++}`;
  }

  #replace(next: PageObject): boolean {
    return this.#commit(this.#objects.map((object) => (object.id === next.id ? next : object)));
  }

  #commit(next: readonly PageObject[]): boolean {
    this.#past.push(this.#objects);
    this.#future = [];
    this.#objects = next;
    this.#emit();
    return true;
  }

  #emit() {
    for (const listener of this.#listeners) listener();
  }
}
