// The page grid's edit model: an ordered list of pages with rotation and deletion, an
// undo/redo history, and the translation of the net result into `pdf.*` tool calls.
import { createStore, type StoreApi } from "zustand/vanilla";

export type PageRotation = 0 | 90 | 180 | 270;
export type RotationStep = 90 | -90 | 180;

/** One page of the working copy. `page` is the page's one-based number in the source file. */
export interface PageEdit {
  page: number;
  rotation: PageRotation;
  deleted: boolean;
}

export interface PageToolCall {
  toolId: "pdf.rotate" | "pdf.delete_pages" | "pdf.reorder";
  params: Record<string, unknown>;
}

export interface PageEditState {
  pageCount: number;
  /** Current order, including deleted pages. */
  pages: PageEdit[];
  past: PageEdit[][];
  future: PageEdit[][];
  /** Selected source page numbers. */
  selected: ReadonlySet<number>;
  /** Anchor of range selection, a source page number. */
  anchor: number | null;
  move(from: number, to: number): void;
  /** Moves a page by a number of positions, for keyboard reordering. */
  nudge(page: number, offset: number): number;
  rotate(pages: readonly number[], step: RotationStep): void;
  remove(pages: readonly number[]): void;
  restore(pages: readonly number[]): void;
  select(page: number, mode: "toggle" | "range"): void;
  selectRange(first: number, last: number): void;
  selectAll(): void;
  clearSelection(): void;
  undo(): void;
  redo(): void;
  /** Discards every pending edit and the history. */
  reset(): void;
}

export type PageEditStore = StoreApi<PageEditState>;

const identity = (pageCount: number): PageEdit[] =>
  Array.from({ length: pageCount }, (_, index) => ({
    page: index + 1,
    rotation: 0 as PageRotation,
    deleted: false,
  }));

const rotated = (rotation: PageRotation, step: RotationStep): PageRotation =>
  ((((rotation + step) % 360) + 360) % 360) as PageRotation;

function sameEdits(left: readonly PageEdit[], right: readonly PageEdit[]) {
  return (
    left.length === right.length &&
    left.every(
      (entry, index) =>
        entry.page === right[index]?.page &&
        entry.rotation === right[index]?.rotation &&
        entry.deleted === right[index]?.deleted,
    )
  );
}

export function createPageEditStore(pageCount: number): PageEditStore {
  return createStore<PageEditState>((set, get) => {
    /** Records the current pages in the history and swaps in the edited ones. */
    const commit = (next: PageEdit[]) => {
      const { pages, past } = get();
      if (sameEdits(pages, next)) return;
      set({ pages: next, past: [...past, pages], future: [] });
    };
    const apply = (targets: readonly number[], change: (entry: PageEdit) => PageEdit) => {
      const wanted = new Set(targets);
      commit(get().pages.map((entry) => (wanted.has(entry.page) ? change(entry) : entry)));
    };
    return {
      pageCount,
      pages: identity(pageCount),
      past: [],
      future: [],
      selected: new Set(),
      anchor: null,
      move(from, to) {
        const { pages } = get();
        if (from === to || from < 0 || to < 0 || from >= pages.length || to >= pages.length) return;
        const next = [...pages];
        const [entry] = next.splice(from, 1);
        if (entry) next.splice(to, 0, entry);
        commit(next);
      },
      nudge(page, offset) {
        const from = get().pages.findIndex((entry) => entry.page === page);
        const to = Math.min(Math.max(from + offset, 0), get().pages.length - 1);
        if (from >= 0) get().move(from, to);
        return to;
      },
      rotate(pages, step) {
        apply(pages, (entry) => ({ ...entry, rotation: rotated(entry.rotation, step) }));
      },
      remove(pages) {
        apply(pages, (entry) => ({ ...entry, deleted: true }));
      },
      restore(pages) {
        apply(pages, (entry) => ({ ...entry, deleted: false }));
      },
      select(page, mode) {
        const { selected, anchor } = get();
        if (mode === "range" && anchor !== null) {
          get().selectRange(anchor, page);
          return;
        }
        const next = new Set(selected);
        if (next.has(page)) next.delete(page);
        else next.add(page);
        set({ selected: next, anchor: page });
      },
      selectRange(first, last) {
        const order = get().pages.map((entry) => entry.page);
        const a = order.indexOf(first);
        const b = order.indexOf(last);
        if (a < 0 || b < 0) return;
        const range = order.slice(Math.min(a, b), Math.max(a, b) + 1);
        set({ selected: new Set([...get().selected, ...range]), anchor: last });
      },
      selectAll() {
        set({ selected: new Set(get().pages.map((entry) => entry.page)) });
      },
      clearSelection() {
        set({ selected: new Set(), anchor: null });
      },
      undo() {
        const { past, pages, future } = get();
        const previous = past.at(-1);
        if (previous) set({ pages: previous, past: past.slice(0, -1), future: [pages, ...future] });
      },
      redo() {
        const { past, pages, future } = get();
        const next = future[0];
        if (next) set({ pages: next, past: [...past, pages], future: future.slice(1) });
      },
      reset() {
        set({
          pages: identity(get().pageCount),
          past: [],
          future: [],
          selected: new Set(),
          anchor: null,
        });
      },
    };
  });
}

/** True when the working copy differs from the source file. */
export function isDirty(state: Pick<PageEditState, "pages" | "pageCount">): boolean {
  return !sameEdits(state.pages, identity(state.pageCount));
}

export function remainingPages(pages: readonly PageEdit[]): number {
  return pages.filter((entry) => !entry.deleted).length;
}

/** Formats numbers as a page range: `1-3,5`. Runs collapse only when they ascend by one. */
export function toPageList(numbers: readonly number[], { sort }: { sort: boolean }): string {
  const list = sort ? [...new Set(numbers)].sort((a, b) => a - b) : [...numbers];
  const parts: string[] = [];
  for (let start = 0; start < list.length; ) {
    let end = start;
    while (end + 1 < list.length && (list[end + 1] as number) === (list[end] as number) + 1)
      end += 1;
    parts.push(end > start ? `${list[start]}-${list[end]}` : String(list[start]));
    start = end + 1;
  }
  return parts.join(",");
}

/**
 * The tool calls that turn the source file into the working copy, in execution order:
 * rotate and delete address source page numbers, so they run first; reorder then addresses
 * the numbering of the pages that remain.
 */
export function planPageEdits(pages: readonly PageEdit[]): PageToolCall[] {
  const kept = pages.filter((entry) => !entry.deleted);
  if (kept.length === 0) throw new Error("A document needs at least one page.");
  const calls: PageToolCall[] = [];
  for (const angle of [90, 180, 270] as const) {
    const targets = kept.filter((entry) => entry.rotation === angle).map((entry) => entry.page);
    if (targets.length)
      calls.push({
        toolId: "pdf.rotate",
        params: { angle, pages: toPageList(targets, { sort: true }) },
      });
  }
  const removed = pages.filter((entry) => entry.deleted).map((entry) => entry.page);
  if (removed.length)
    calls.push({
      toolId: "pdf.delete_pages",
      params: { pages: toPageList(removed, { sort: true }) },
    });
  const survivors = [...kept].sort((a, b) => a.page - b.page).map((entry) => entry.page);
  const sequence = kept.map((entry) => survivors.indexOf(entry.page) + 1);
  if (sequence.some((value, index) => value !== index + 1))
    calls.push({ toolId: "pdf.reorder", params: { order: toPageList(sequence, { sort: false }) } });
  return calls;
}
