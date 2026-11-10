"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { PageThumbnail } from "@ariadshift/ui/components/page-thumbnail";
import {
  draggable,
  dropTargetForElements,
} from "@atlaskit/pragmatic-drag-and-drop/element/adapter";
import { observeElementRect, useVirtualizer } from "@tanstack/react-virtual";
import {
  CheckIcon,
  EraserIcon,
  RedoIcon,
  RotateCcwIcon,
  RotateCwIcon,
  Trash2Icon,
  Undo2Icon,
} from "lucide-react";
import { type KeyboardEvent, type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { useStore } from "zustand";
import { useWorkspaceCopy } from "./copy";
import {
  createPageEditStore,
  isDirty,
  type PageEdit,
  type PageEditStore,
  type PageToolCall,
  planPageEdits,
  remainingPages,
} from "./page-edits";

/** Pages beyond this count are laid out in virtual rows instead of one DOM card each. */
export const VIRTUALIZE_AFTER = 120;
const CARD_WIDTH = 168;
const ROW_HEIGHT = 268;

export interface PageGridProps {
  pageCount: number;
  /** One-based page numbers the insights flagged. */
  blankPages?: readonly number[];
  scannedPages?: readonly number[];
  /** Renders the preview of a one-based page. */
  renderPreview?: (page: number) => ReactNode;
  /** Runs the planned `pdf.*` calls, in order, through the tool runner. */
  onApply: (calls: PageToolCall[]) => void | Promise<void>;
  busy?: boolean;
  /** Supplies the edit store, so a parent can observe or reset it. */
  store?: PageEditStore;
  /** Rows laid out before the first measurement; tests use it, browsers measure. */
  initialRect?: { width: number; height: number };
}

export function PageGrid({
  pageCount,
  blankPages = [],
  scannedPages = [],
  renderPreview,
  onApply,
  busy = false,
  store: provided,
  initialRect,
}: PageGridProps) {
  const copy = useWorkspaceCopy().pageGrid;
  const ownStore = useMemo(() => provided ?? createPageEditStore(pageCount), [provided, pageCount]);
  const state = useStore(ownStore);
  const [announcement, setAnnouncement] = useState("");
  const [rangeText, setRangeText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const shift = useRef(false);

  const selected = [...state.selected];
  const kept = remainingPages(state.pages);
  const dirty = isDirty(state);
  const blank = useMemo(() => new Set(blankPages), [blankPages]);
  const scanned = useMemo(() => new Set(scannedPages), [scannedPages]);

  async function apply() {
    setError(null);
    try {
      await onApply(planPageEdits(state.pages));
      state.reset();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    }
  }

  function selectRangeText() {
    const match = /^\s*(\d+)\s*(?:-\s*(\d+))?\s*$/.exec(rangeText);
    if (!match) return;
    const first = Number(match[1]);
    const last = Number(match[2] ?? match[1]);
    const [low, high] = first <= last ? [first, last] : [last, first];
    state.selectRange(Math.max(1, low), Math.min(pageCount, high));
    setRangeText("");
  }

  const cardProps = (entry: PageEdit, position: number) => ({
    entry,
    position,
    total: state.pages.length,
    selected: state.selected.has(entry.page),
    blank: blank.has(entry.page),
    scanned: scanned.has(entry.page),
    preview: renderPreview?.(entry.page),
    busy,
    onSelect: (mode: "toggle" | "range") => state.select(entry.page, mode),
    onRotate: (step: 90 | -90) => state.rotate([entry.page], step),
    onRemove: () => state.remove([entry.page]),
    onRestore: () => state.restore([entry.page]),
    onDropFrom: (from: number) => state.move(from, position),
    onNudge: (offset: number) => {
      const index = state.nudge(entry.page, offset);
      setAnnouncement(copy.moved(entry.page, index + 1));
    },
    shift,
  });

  return (
    <section data-slot="page-grid" aria-label={copy.title} className="flex min-w-0 flex-col gap-3">
      <div role="toolbar" aria-label={copy.toolbar} className="flex flex-wrap items-center gap-2">
        <Button
          variant="outline"
          size="sm"
          disabled={busy || state.past.length === 0}
          onClick={state.undo}
        >
          <Undo2Icon aria-hidden="true" />
          {copy.undo}
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={busy || state.future.length === 0}
          onClick={state.redo}
        >
          <RedoIcon aria-hidden="true" />
          {copy.redo}
        </Button>
        <Button variant="ghost" size="sm" disabled={busy || !dirty} onClick={state.reset}>
          <EraserIcon aria-hidden="true" />
          {copy.reset}
        </Button>
        <Badge variant="secondary">{copy.pending(state.past.length)}</Badge>
        <span className="mx-1 h-5 w-px bg-border" aria-hidden="true" />
        <Button variant="ghost" size="sm" onClick={state.selectAll}>
          {copy.selectAll}
        </Button>
        <Button
          variant="ghost"
          size="sm"
          disabled={selected.length === 0}
          onClick={state.clearSelection}
        >
          {copy.clearSelection}
        </Button>
        <form
          className="flex items-center gap-1"
          onSubmit={(event) => {
            event.preventDefault();
            selectRangeText();
          }}
        >
          <label className="sr-only" htmlFor="page-grid-range">
            {copy.selectRange}
          </label>
          <input
            id="page-grid-range"
            value={rangeText}
            placeholder={copy.rangePlaceholder}
            onChange={(event) => setRangeText(event.target.value)}
            className="h-7 w-24 rounded-lg border bg-background px-2 text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
          />
          <Button type="submit" variant="outline" size="sm" disabled={rangeText.trim() === ""}>
            {copy.selectRange}
          </Button>
        </form>
        <span className="mx-1 h-5 w-px bg-border" aria-hidden="true" />
        <Button
          variant="outline"
          size="sm"
          disabled={busy || selected.length === 0}
          onClick={() => state.rotate(selected, -90)}
        >
          <RotateCcwIcon aria-hidden="true" />
          {copy.rotateLeft}
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={busy || selected.length === 0}
          onClick={() => state.rotate(selected, 90)}
        >
          <RotateCwIcon aria-hidden="true" />
          {copy.rotateRight}
        </Button>
        <Button
          variant="destructive"
          size="sm"
          disabled={busy || selected.length === 0}
          onClick={() => state.remove(selected)}
        >
          <Trash2Icon aria-hidden="true" />
          {copy.deleteSelected}
        </Button>
        <Button
          variant="outline"
          size="sm"
          disabled={busy || selected.length === 0}
          onClick={() => state.restore(selected)}
        >
          {copy.restoreSelected}
        </Button>
        <Button
          size="sm"
          className="ms-auto"
          disabled={busy || !dirty || kept === 0}
          onClick={apply}
        >
          <CheckIcon aria-hidden="true" />
          {copy.apply}
        </Button>
      </div>

      <p className="text-xs text-muted-foreground">{copy.hint}</p>
      {kept === 0 && (
        <p role="alert" className="text-sm text-destructive">
          {copy.cannotDeleteAll}
        </p>
      )}
      {error && (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      )}
      <p role="status" aria-live="polite" className="sr-only">
        {announcement}
      </p>

      {state.pages.length > VIRTUALIZE_AFTER ? (
        <VirtualGrid pages={state.pages} initialRect={initialRect} card={cardProps} />
      ) : (
        <ul className="m-0 grid list-none grid-cols-[repeat(auto-fill,minmax(10rem,1fr))] gap-3 p-0">
          {state.pages.map((entry, position) => (
            <li key={entry.page}>
              <PageCard {...cardProps(entry, position)} />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

interface CardProps {
  entry: PageEdit;
  position: number;
  total: number;
  selected: boolean;
  blank: boolean;
  scanned: boolean;
  preview: ReactNode;
  busy: boolean;
  onSelect: (mode: "toggle" | "range") => void;
  onRotate: (step: 90 | -90) => void;
  onRemove: () => void;
  onRestore: () => void;
  /** Called with the position of the page dropped on this card; it takes this card's place. */
  onDropFrom: (from: number) => void;
  onNudge: (offset: number) => void;
  shift: { current: boolean };
}

/** Lays the pages out in rows of whole cards and mounts only the rows near the viewport. */
function VirtualGrid({
  pages,
  card,
  initialRect,
}: {
  pages: PageEdit[];
  card: (entry: PageEdit, position: number) => CardProps;
  initialRect?: { width: number; height: number };
}) {
  const host = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(initialRect?.width ?? 960);
  useEffect(() => {
    const node = host.current;
    if (!node || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(([entry]) => {
      if (entry) setWidth(entry.contentRect.width);
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  const columns = Math.max(1, Math.floor(width / CARD_WIDTH));
  const rows = Math.ceil(pages.length / columns);
  const virtualizer = useVirtualizer({
    count: rows,
    getScrollElement: () => host.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 3,
    initialRect: initialRect ?? { width, height: 640 },
    // An unmeasured (zero-sized) scroll area falls back to the initial size, so rows render
    // in environments without layout.
    observeElementRect: (instance, callback) =>
      observeElementRect(instance, (rect) =>
        callback(rect.height === 0 && initialRect ? initialRect : rect),
      ),
  });
  return (
    <div ref={host} className="max-h-[70vh] overflow-auto" data-slot="page-grid-virtual">
      <ul className="relative m-0 list-none p-0" style={{ height: virtualizer.getTotalSize() }}>
        {virtualizer.getVirtualItems().map((row) => (
          <li
            key={row.key}
            className="absolute inset-x-0 grid gap-3"
            style={{
              top: 0,
              transform: `translateY(${row.start}px)`,
              height: row.size,
              gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
            }}
          >
            {pages
              .slice(row.index * columns, row.index * columns + columns)
              .map((entry, offset) => (
                <PageCard key={entry.page} {...card(entry, row.index * columns + offset)} />
              ))}
          </li>
        ))}
      </ul>
    </div>
  );
}

function PageCard({
  entry,
  position,
  total,
  selected,
  blank,
  scanned,
  preview,
  busy,
  onSelect,
  onRotate,
  onRemove,
  onRestore,
  onDropFrom,
  onNudge,
  shift,
}: CardProps) {
  const copy = useWorkspaceCopy().pageGrid;
  const card = useRef<HTMLDivElement>(null);
  const handle = useRef<HTMLButtonElement>(null);
  const [dragging, setDragging] = useState(false);
  const [over, setOver] = useState(false);
  // The adapter callbacks run outside React; keep the latest values reachable.
  const latest = useRef({ position, onDropFrom });
  latest.current = { position, onDropFrom };

  useEffect(() => {
    const element = card.current;
    const grip = handle.current;
    if (!(element && grip) || busy) return;
    const stopDrag = draggable({
      element,
      dragHandle: grip,
      getInitialData: () => ({ page: entry.page, position: latest.current.position }),
      onDragStart: () => setDragging(true),
      onDrop: () => setDragging(false),
    });
    const stopDrop = dropTargetForElements({
      element,
      getData: () => ({ page: entry.page }),
      canDrop: ({ source }) => source.data.page !== entry.page,
      onDragEnter: () => setOver(true),
      onDragLeave: () => setOver(false),
      onDrop: ({ source }) => {
        setOver(false);
        const from = Number(source.data.position);
        if (Number.isInteger(from)) latest.current.onDropFrom(from);
      },
    });
    return () => {
      stopDrag();
      stopDrop();
    };
  }, [entry.page, busy]);

  function onKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    const rtl =
      (event.currentTarget.closest("[dir]")?.getAttribute("dir") ?? document.dir) === "rtl";
    const forward = rtl ? "ArrowLeft" : "ArrowRight";
    const backward = rtl ? "ArrowRight" : "ArrowLeft";
    if (event.key === forward || event.key === "ArrowDown") onNudge(1);
    else if (event.key === backward || event.key === "ArrowUp") onNudge(-1);
    else return;
    event.preventDefault();
    // The card re-renders in its new place; keep focus on the same handle.
    setTimeout(() => handle.current?.focus(), 0);
  }

  return (
    <div
      ref={card}
      className={over ? "rounded-lg ring-2 ring-thread" : undefined}
      data-over={over || undefined}
    >
      <PageThumbnail
        page={entry.page}
        selected={selected}
        rotation={entry.rotation}
        dragging={dragging}
        blank={blank}
        scanned={scanned}
        data-deleted={entry.deleted || undefined}
        className={entry.deleted ? "opacity-50" : undefined}
        labels={{
          page: copy.cardPage,
          select: copy.cardSelect,
          rotate: copy.cardRotate,
          remove: copy.cardDelete,
          drag: copy.cardMove,
          blank: copy.blank,
          scanned: copy.scanned,
        }}
        onClickCapture={(event) => {
          shift.current = event.shiftKey;
        }}
        onSelectedChange={() => onSelect(shift.current ? "range" : "toggle")}
        onRotate={entry.deleted ? undefined : () => onRotate(90)}
        onDelete={entry.deleted ? undefined : onRemove}
        dragHandleProps={{
          ref: handle,
          disabled: busy,
          onKeyDown,
          title: copy.dragHint,
          "aria-posinset": position + 1,
          "aria-setsize": total,
        }}
      >
        {preview}
      </PageThumbnail>
      {entry.deleted && (
        <Button variant="outline" size="xs" className="mt-1 w-full" onClick={onRestore}>
          {copy.cardRestore(entry.page)}
        </Button>
      )}
    </div>
  );
}
