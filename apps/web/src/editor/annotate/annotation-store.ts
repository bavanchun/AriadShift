// The annotation model of the Annotate surface: pending annotations placed on pages, an
// undo/redo history over placement, edits and deletions, and the translation of the result
// into `pdf.annotations` tool calls. Coordinates are PDF points with a bottom-left origin and
// 1-based page numbers, exactly what the tool schema expects.
import { createStore, type StoreApi } from "zustand/vanilla";
import type {
  AnnotationItem,
  AnnotationsParams,
  AnnotationType,
  EditorToolCall,
  StampName,
} from "./tool-params";

export type Point = readonly [number, number];
/** `[left, bottom, right, top]`. */
export type Rect = readonly [number, number, number, number];

export type AnnotationKind =
  | "highlight"
  | "underline"
  | "strike"
  | "note"
  | "ink"
  | "shape"
  | "stamp";
export type AnnotationTool = AnnotationKind | "select";
export type ShapeKind = "rectangle" | "ellipse" | "line" | "arrow";
export type NoteIcon = "comment" | "note" | "key" | "help" | "insert";

export const HIGHLIGHT_COLOURS = {
  yellow: "#FFEB3B",
  green: "#66BB6A",
  blue: "#42A5F5",
  pink: "#F48FB1",
  orange: "#FFA726",
} as const;
export type HighlightColourName = keyof typeof HIGHLIGHT_COLOURS;

export const NOTE_ICONS: readonly NoteIcon[] = ["comment", "note", "key", "help", "insert"];
export const SHAPE_KINDS: readonly ShapeKind[] = ["rectangle", "ellipse", "line", "arrow"];

/** Preset badges. The ISO standard stamps map to a stamp name; the others are drawn as text. */
export const STAMP_PRESETS = ["APPROVED", "DRAFT", "CONFIDENTIAL", "REVIEWED", "VOID"] as const;
const STAMP_NAMES: Readonly<Record<string, StampName>> = {
  APPROVED: "approved",
  DRAFT: "draft",
  CONFIDENTIAL: "confidential",
};

interface AnnotationBase {
  id: string;
  /** One-based page number. */
  page: number;
  /** `#rrggbb`. */
  color: string;
  opacity: number;
}

export interface MarkupAnnotation extends AnnotationBase {
  kind: "highlight" | "underline" | "strike";
  rect: Rect;
}

export interface NoteAnnotation extends AnnotationBase {
  kind: "note";
  rect: Rect;
  author: string;
  contents: string;
  icon: NoteIcon;
}

export interface InkAnnotation extends AnnotationBase {
  kind: "ink";
  strokes: Point[][];
  lineWidth: number;
}

export interface ShapeAnnotation extends AnnotationBase {
  kind: "shape";
  shape: ShapeKind;
  /** The bounding box; for a line or arrow it spans `from` and `to`. */
  rect: Rect;
  from: Point;
  to: Point;
  fill: string | null;
  lineWidth: number;
}

export interface StampAnnotation extends AnnotationBase {
  kind: "stamp";
  rect: Rect;
  /** A preset badge or free text. */
  label: string;
}

export type Annotation =
  | MarkupAnnotation
  | NoteAnnotation
  | InkAnnotation
  | ShapeAnnotation
  | StampAnnotation;

type DistributiveOmit<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;
export type AnnotationDraft = DistributiveOmit<Annotation, "id">;
/** Fields an edit may change; `id` and `kind` are fixed once placed. */
export type AnnotationPatch = Partial<DistributiveOmit<Annotation, "id" | "kind">>;

/** An annotation already stored in the PDF, which the surface can only remove. */
export interface ExistingAnnotation {
  id: string;
  page: number;
  type: AnnotationType;
  rect?: Rect;
}

export interface AnnotationSettings {
  tool: AnnotationTool;
  page: number;
  color: string;
  opacity: number;
  lineWidth: number;
  shape: ShapeKind;
  fill: string | null;
  stamp: string;
  author: string;
  icon: NoteIcon;
}

interface Snapshot {
  items: readonly Annotation[];
  /** Ids of existing annotations marked for removal. */
  removed: readonly string[];
}

export interface AnnotationState extends Snapshot {
  existing: readonly ExistingAnnotation[];
  past: readonly Snapshot[];
  future: readonly Snapshot[];
  selectedId: string | null;
  settings: AnnotationSettings;
  setTool(tool: AnnotationTool): void;
  configure(patch: Partial<Omit<AnnotationSettings, "tool">>): void;
  setExisting(existing: readonly ExistingAnnotation[]): void;
  /** Places an annotation and selects it. Returns its id. */
  add(draft: AnnotationDraft): string;
  /** Edits a pending annotation; a no-op edit leaves the history untouched. */
  update(id: string, patch: AnnotationPatch): void;
  /** Moves a pending annotation by a distance in points. */
  move(id: string, dx: number, dy: number): void;
  /** Deletes a pending annotation, or marks an existing one for removal. */
  remove(id: string): void;
  restoreExisting(id: string): void;
  select(id: string | null): void;
  undo(): void;
  redo(): void;
  /** Discards every pending change and the history. */
  reset(): void;
}

export type AnnotationStore = StoreApi<AnnotationState>;

const TOOL_DEFAULTS: Readonly<
  Record<AnnotationKind, { color: string; opacity: number; lineWidth: number }>
> = {
  highlight: { color: HIGHLIGHT_COLOURS.yellow, opacity: 0.4, lineWidth: 1 },
  underline: { color: "#E53935", opacity: 1, lineWidth: 1 },
  strike: { color: "#E53935", opacity: 1, lineWidth: 1 },
  note: { color: "#FFD54F", opacity: 1, lineWidth: 1 },
  ink: { color: "#1E88E5", opacity: 1, lineWidth: 2 },
  shape: { color: "#E53935", opacity: 1, lineWidth: 2 },
  stamp: { color: "#C62828", opacity: 1, lineWidth: 1 },
};

const DEFAULT_SIZE: Readonly<Record<AnnotationKind, readonly [number, number]>> = {
  highlight: [120, 14],
  underline: [120, 14],
  strike: [120, 14],
  note: [24, 24],
  ink: [0, 0],
  shape: [100, 60],
  stamp: [140, 44],
};

/** Smallest drag, in points, that counts as drawing a box instead of a click. */
const MIN_DRAG = 4;

export const DEFAULT_SETTINGS: AnnotationSettings = {
  tool: "select",
  page: 1,
  color: TOOL_DEFAULTS.highlight.color,
  opacity: TOOL_DEFAULTS.highlight.opacity,
  lineWidth: 2,
  shape: "rectangle",
  fill: null,
  stamp: "APPROVED",
  author: "",
  icon: "comment",
};

export const normaliseRect = (a: Point, b: Point): Rect => [
  Math.min(a[0], b[0]),
  Math.min(a[1], b[1]),
  Math.max(a[0], b[0]),
  Math.max(a[1], b[1]),
];

const boundsOfPoints = (points: readonly Point[]): Rect =>
  points.length === 0
    ? [0, 0, 0, 0]
    : [
        Math.min(...points.map((p) => p[0])),
        Math.min(...points.map((p) => p[1])),
        Math.max(...points.map((p) => p[0])),
        Math.max(...points.map((p) => p[1])),
      ];

/** The box an annotation occupies, for selection outlines and hit areas. */
export function boundsOf(annotation: Annotation): Rect {
  if (annotation.kind === "ink") {
    const [left, bottom, right, top] = boundsOfPoints(annotation.strokes.flat());
    const pad = annotation.lineWidth / 2;
    return [left - pad, bottom - pad, right + pad, top + pad];
  }
  return annotation.rect;
}

function translate(annotation: Annotation, dx: number, dy: number): Annotation {
  const shift = (p: Point): Point => [p[0] + dx, p[1] + dy];
  const shiftRect = (r: Rect): Rect => [r[0] + dx, r[1] + dy, r[2] + dx, r[3] + dy];
  switch (annotation.kind) {
    case "ink":
      return { ...annotation, strokes: annotation.strokes.map((stroke) => stroke.map(shift)) };
    case "shape":
      return {
        ...annotation,
        rect: shiftRect(annotation.rect),
        from: shift(annotation.from),
        to: shift(annotation.to),
      };
    default:
      return { ...annotation, rect: shiftRect(annotation.rect) };
  }
}

/** What a pointer gesture produced on a page: a click, a drag between two points, or a stroke. */
export type Gesture =
  | { type: "drag"; from: Point; to: Point }
  | { type: "stroke"; points: readonly Point[] };

/**
 * Builds the annotation a gesture places with the current tool. A click, or a drag shorter
 * than a few points, gets the tool's default size centred on the pointer, so keyboard and
 * pointer users reach the same result. Returns `null` for the select tool and for empty strokes.
 */
export function draftFromGesture(
  settings: AnnotationSettings,
  gesture: Gesture,
): AnnotationDraft | null {
  const { tool, page, color, opacity } = settings;
  if (tool === "select") return null;
  const base = { page, color, opacity };

  if (tool === "ink") {
    if (gesture.type !== "stroke" || gesture.points.length < 2) return null;
    return { ...base, kind: "ink", strokes: [[...gesture.points]], lineWidth: settings.lineWidth };
  }
  if (gesture.type !== "drag") return null;

  const { from, to } = gesture;
  const dragged = Math.hypot(to[0] - from[0], to[1] - from[1]) >= MIN_DRAG;
  const [width, height] = DEFAULT_SIZE[tool];
  const rect: Rect = dragged
    ? normaliseRect(from, to)
    : [from[0] - width / 2, from[1] - height / 2, from[0] + width / 2, from[1] + height / 2];

  switch (tool) {
    case "highlight":
    case "underline":
    case "strike":
      return { ...base, kind: tool, rect };
    case "note":
      return {
        ...base,
        kind: "note",
        rect: [from[0], from[1] - height, from[0] + width, from[1]],
        author: settings.author,
        contents: "",
        icon: settings.icon,
      };
    case "shape": {
      const line = settings.shape === "line" || settings.shape === "arrow";
      const start: Point = line && dragged ? from : [rect[0], rect[1]];
      const end: Point = line && dragged ? to : [rect[2], rect[3]];
      return {
        ...base,
        kind: "shape",
        shape: settings.shape,
        rect: line ? normaliseRect(start, end) : rect,
        from: start,
        to: end,
        fill: settings.shape === "line" || settings.shape === "arrow" ? null : settings.fill,
        lineWidth: settings.lineWidth,
      };
    }
    case "stamp":
      return { ...base, kind: "stamp", rect, label: settings.stamp };
  }
}

const sameValue = (left: unknown, right: unknown) => JSON.stringify(left) === JSON.stringify(right);

export function createAnnotationStore(initial: Partial<AnnotationSettings> = {}): AnnotationStore {
  let counter = 0;
  const nextId = () => `annotation-${++counter}`;

  return createStore<AnnotationState>()((set, get) => {
    const snapshot = (): Snapshot => ({ items: get().items, removed: get().removed });
    const commit = (next: Snapshot, extra: Partial<AnnotationState> = {}) =>
      set((state) => ({ ...next, ...extra, past: [...state.past, snapshot()], future: [] }));

    return {
      items: [],
      removed: [],
      existing: [],
      past: [],
      future: [],
      selectedId: null,
      settings: { ...DEFAULT_SETTINGS, ...initial },
      setTool(tool) {
        set((state) => {
          const defaults = tool === "select" ? null : TOOL_DEFAULTS[tool];
          return {
            settings: {
              ...state.settings,
              tool,
              ...(defaults
                ? {
                    color: defaults.color,
                    opacity: defaults.opacity,
                    lineWidth: tool === "ink" || tool === "shape" ? defaults.lineWidth : 1,
                  }
                : {}),
            },
          };
        });
      },
      configure(patch) {
        set((state) => ({ settings: { ...state.settings, ...patch } }));
      },
      setExisting(existing) {
        set({ existing, removed: [], items: [], past: [], future: [], selectedId: null });
      },
      add(draft) {
        const id = nextId();
        commit(
          { items: [...get().items, { ...draft, id } as Annotation], removed: get().removed },
          {
            selectedId: id,
          },
        );
        return id;
      },
      update(id, patch) {
        const current = get().items.find((item) => item.id === id);
        if (!current) return;
        const next = { ...current, ...patch, id, kind: current.kind } as Annotation;
        if (sameValue(next, current)) return;
        commit({
          items: get().items.map((item) => (item.id === id ? next : item)),
          removed: get().removed,
        });
      },
      move(id, dx, dy) {
        const current = get().items.find((item) => item.id === id);
        if (!current || (dx === 0 && dy === 0)) return;
        const next = translate(current, dx, dy);
        commit({
          items: get().items.map((item) => (item.id === id ? next : item)),
          removed: get().removed,
        });
      },
      remove(id) {
        const { items, existing, removed } = get();
        if (items.some((item) => item.id === id)) {
          commit(
            { items: items.filter((item) => item.id !== id), removed },
            get().selectedId === id ? { selectedId: null } : {},
          );
        } else if (existing.some((entry) => entry.id === id) && !removed.includes(id)) {
          commit({ items, removed: [...removed, id] });
        }
      },
      restoreExisting(id) {
        const { items, removed } = get();
        if (removed.includes(id))
          commit({ items, removed: removed.filter((value) => value !== id) });
      },
      select(id) {
        set({ selectedId: id });
      },
      undo() {
        const { past, future } = get();
        const previous = past.at(-1);
        if (!previous) return;
        const selectedId = previous.items.some((item) => item.id === get().selectedId)
          ? get().selectedId
          : null;
        set({
          ...previous,
          past: past.slice(0, -1),
          future: [snapshot(), ...future],
          selectedId,
        });
      },
      redo() {
        const { past, future } = get();
        const [next, ...rest] = future;
        if (!next) return;
        set({ ...next, past: [...past, snapshot()], future: rest });
      },
      reset() {
        set({ items: [], removed: [], past: [], future: [], selectedId: null });
      },
    };
  });
}

/** Number of pending changes: new annotations plus removals of existing ones. */
export const pendingCount = (state: Pick<AnnotationState, "items" | "removed">): number =>
  state.items.length + state.removed.length;

const quadOf = ([left, bottom, right, top]: Rect): number[] => [
  left,
  top,
  right,
  top,
  left,
  bottom,
  right,
  bottom,
];

const MARKUP_TYPE = {
  highlight: "highlight",
  underline: "underline",
  strike: "strikeout",
} as const;

/** The standard stamp a preset label maps to; other labels are drawn from their text. */
export const stampNameOf = (label: string): StampName | undefined =>
  STAMP_NAMES[label.trim().toUpperCase()];

function toItem(annotation: Annotation): AnnotationItem {
  const common = {
    page: annotation.page,
    color: annotation.color,
    opacity: annotation.opacity,
  };
  switch (annotation.kind) {
    case "highlight":
    case "underline":
    case "strike":
      return {
        ...common,
        type: MARKUP_TYPE[annotation.kind],
        quads: [quadOf(annotation.rect)],
      };
    case "note": {
      const item: AnnotationItem = {
        ...common,
        type: "note",
        rect: [...annotation.rect],
        // The icon preset only styles the on-page marker: `pdf.annotations` has no icon field.
        contents: annotation.contents,
      };
      if (annotation.author) item.author = annotation.author;
      return item;
    }
    case "ink":
      return {
        ...common,
        type: "ink",
        strokes: annotation.strokes.map((stroke) => stroke.map((p) => [p[0], p[1]])),
        line_width: annotation.lineWidth,
      };
    case "shape": {
      const line = annotation.shape === "line" || annotation.shape === "arrow";
      const item: AnnotationItem = {
        ...common,
        type: annotation.shape,
        line_width: annotation.lineWidth,
      };
      if (line) {
        item.points = [
          [annotation.from[0], annotation.from[1]],
          [annotation.to[0], annotation.to[1]],
        ];
      } else {
        item.rect = [...annotation.rect];
        if (annotation.fill) item.fill = annotation.fill;
      }
      return item;
    }
    case "stamp": {
      const item: AnnotationItem = { ...common, type: "stamp", rect: [...annotation.rect] };
      const name = stampNameOf(annotation.label);
      if (name) item.stamp = name;
      else item.text = annotation.label;
      return item;
    }
  }
}

/** The `pdf.annotations` parameters that add every pending annotation. */
export function toAnnotateParams(items: readonly Annotation[]): AnnotationsParams {
  return { action: "add", items: items.map(toItem) };
}

/** The `pdf.annotations` parameters that delete annotations already stored in the PDF. */
export function toAnnotationRemoveParams(ids: readonly string[]): AnnotationsParams {
  return { action: "delete", ids: [...ids] };
}

/** The tool calls for the net result: one add, then one delete, each only when needed. */
export function planAnnotationCalls(
  state: Pick<AnnotationState, "items" | "removed">,
): EditorToolCall[] {
  const calls: EditorToolCall[] = [];
  if (state.items.length > 0)
    calls.push({ toolId: "pdf.annotations", params: toAnnotateParams(state.items) });
  if (state.removed.length > 0)
    calls.push({ toolId: "pdf.annotations", params: toAnnotationRemoveParams(state.removed) });
  return calls;
}
