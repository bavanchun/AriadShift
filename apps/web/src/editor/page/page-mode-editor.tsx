"use client";

import { Alert, AlertDescription } from "@ariadshift/ui/components/alert";
import { Button } from "@ariadshift/ui/components/button";
import {
  CheckIcon,
  ChevronLeftIcon,
  ChevronRightIcon,
  ImagePlusIcon,
  MousePointer2Icon,
  Redo2Icon,
  TextCursorInputIcon,
  Trash2Icon,
  TypeIcon,
  Undo2Icon,
} from "lucide-react";
import {
  type KeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
  type PointerEvent as ReactPointerEvent,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { FontSubstitutionAlert } from "./font-substitution-alert";
import {
  canResize,
  type EditApplyParams,
  MAX_EDIT_OPS,
  PageEditSession,
} from "./page-edit-session";
import { DEFAULT_PAGE_MODE_LABELS, formatLabel, type PageModeLabels } from "./page-mode-labels";
import {
  type FontResolution,
  type PageBBox,
  type PageInfo,
  type PageObject,
  resolveFontForEdit,
} from "./page-model";
import { PageObjectInspector } from "./page-object-inspector";

export type PageTool = "select" | "edit-text" | "add-text";

export interface ImagePayload {
  dataUrl: string;
  /** Pixel size of the decoded bitmap. */
  width: number;
  height: number;
}

export interface PageModeEditorProps {
  pages: PageInfo[];
  /**
   * Objects of every page, as reported by `edit.page_objects`. The editor snapshots them on
   * mount; remount it (change its `key`) to start a new document.
   */
  objects: readonly PageObject[];
  /**
   * Supply a session to keep unapplied edits when the editor unmounts, for example while the host
   * shows another mode. Without one the editor makes its own from `objects`.
   */
  session?: PageEditSession;
  initialPage?: number;
  /** Screen pixels per PDF point. */
  scale?: number;
  /** Receives the `edit.apply` parameters when the user applies their edits. */
  onApply?: (params: EditApplyParams) => void;
  /**
   * Reports the page in view and, when an object is selected, its top-left corner in page points
   * with the origin at the top left (the IR layout's convention), so another mode can open there.
   */
  onLocationChange?: (location: { page: number; xPt?: number; yPt?: number }) => void;
  /** Draws the rendered page underneath the object overlay. */
  renderPageBackground?: (page: PageInfo) => ReactNode;
  /** Decodes a chosen image file; defaults to the browser's FileReader and Image. */
  readImage?: (file: File) => Promise<ImagePayload>;
  labels?: Partial<PageModeLabels>;
  className?: string;
}

interface DragState {
  id: string;
  mode: "move" | "resize";
  originX: number;
  originY: number;
  bbox: PageBBox;
  current: PageBBox;
}

const NUDGE_PT = 1;
const NUDGE_SHIFT_PT = 10;
const DEFAULT_IMAGE_WIDTH_PT = 200;
const MIN_SIZE_PT = 1;

export function readImageInBrowser(file: File): Promise<ImagePayload> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(reader.error ?? new Error("Could not read the image."));
    reader.onload = () => {
      const dataUrl = String(reader.result);
      const image = new Image();
      image.onerror = () => reject(new Error("Could not decode the image."));
      image.onload = () =>
        resolve({ dataUrl, width: image.naturalWidth, height: image.naturalHeight });
      image.src = dataUrl;
    };
    reader.readAsDataURL(file);
  });
}

function shiftBox(bbox: PageBBox, dx: number, dy: number): PageBBox {
  return [bbox[0] + dx, bbox[1] + dy, bbox[2] + dx, bbox[3] + dy];
}

function isTypingTarget(target: EventTarget): boolean {
  return target instanceof HTMLElement && /^(INPUT|TEXTAREA|SELECT)$/.test(target.tagName);
}

/** Page mode: select, edit, move, resize, add and delete the objects of one PDF page in place. */
export function PageModeEditor({
  pages,
  objects,
  session: sessionProp,
  initialPage,
  scale = 1,
  onApply,
  onLocationChange,
  renderPageBackground,
  readImage = readImageInBrowser,
  labels,
  className,
}: PageModeEditorProps) {
  const copy = { ...DEFAULT_PAGE_MODE_LABELS, ...labels };
  const [session] = useState(() => sessionProp ?? new PageEditSession(objects));
  const current = useSyncExternalStore(session.subscribe, session.getSnapshot, session.getSnapshot);

  const [tool, setTool] = useState<PageTool>("select");
  const [pageNumber, setPageNumber] = useState(initialPage ?? pages[0]?.page ?? 1);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(null);
  const [drag, setDrag] = useState<DragState | null>(null);
  const dragRef = useRef<DragState | null>(null);
  const cancelEdit = useRef(false);
  const fileInput = useRef<HTMLInputElement>(null);

  const pageIndex = Math.max(
    0,
    pages.findIndex((entry) => entry.page === pageNumber),
  );
  const info = pages[pageIndex];
  const pageObjects = current.filter((object) => object.page === info?.page);
  const selected = selectedId ? session.get(selectedId) : undefined;
  const reportLocation = useRef(onLocationChange);
  reportLocation.current = onLocationChange;
  const reportedPage = info?.page;
  const reportedHeight = info?.heightPt;
  const reportedBox = selected?.bbox;
  useEffect(() => {
    if (reportedPage === undefined) return;
    if (!(reportedBox && reportedHeight !== undefined)) {
      reportLocation.current?.({ page: reportedPage });
      return;
    }
    reportLocation.current?.({
      page: reportedPage,
      xPt: reportedBox[0],
      yPt: reportedHeight - reportedBox[3],
    });
  }, [reportedPage, reportedHeight, reportedBox]);
  const params = session.toEditPageParams();
  const tooMany = params.ops.length > MAX_EDIT_OPS;

  const resolutions = session.substitutions();
  const draftSource = editing ? session.original(editing.id) : undefined;
  const draft: FontResolution | undefined =
    editing && draftSource?.kind === "text"
      ? resolveFontForEdit(draftSource, editing.text)
      : undefined;
  const alerts = [
    ...(draft?.substituted ? [{ id: editing?.id ?? "draft", resolution: draft }] : []),
    ...resolutions.filter((entry) => entry.id !== editing?.id),
  ];

  const dragging = drag !== null;
  useEffect(() => {
    if (!dragging) return;
    const move = (event: PointerEvent) => {
      const state = dragRef.current;
      if (!state) return;
      const dx = (event.clientX - state.originX) / scale;
      const dy = -(event.clientY - state.originY) / scale;
      const [left, bottom, right, top] = state.bbox;
      const next: PageBBox =
        state.mode === "move"
          ? shiftBox(state.bbox, dx, dy)
          : [
              left,
              Math.min(bottom + dy, top - MIN_SIZE_PT),
              Math.max(right + dx, left + MIN_SIZE_PT),
              top,
            ];
      dragRef.current = { ...state, current: next };
      setDrag(dragRef.current);
    };
    const up = () => {
      const state = dragRef.current;
      dragRef.current = null;
      setDrag(null);
      if (!state) return;
      if (state.mode === "move") {
        session.moveObject(
          state.id,
          state.current[0] - state.bbox[0],
          state.current[1] - state.bbox[1],
        );
      } else {
        session.resizeObject(state.id, state.current);
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
    return () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
    };
  }, [dragging, scale, session]);

  function startDrag(event: ReactPointerEvent, object: PageObject, mode: DragState["mode"]) {
    if (tool !== "select" || event.button > 0) return;
    event.stopPropagation();
    setSelectedId(object.id);
    const state: DragState = {
      id: object.id,
      mode,
      originX: event.clientX,
      originY: event.clientY,
      bbox: object.bbox,
      current: object.bbox,
    };
    dragRef.current = state;
    setDrag(state);
  }

  function selectObject(object: PageObject) {
    setSelectedId(object.id);
    if (tool === "edit-text" && object.kind === "text") startEdit(object);
  }

  function startEdit(object: PageObject) {
    if (object.kind !== "text") return;
    cancelEdit.current = false;
    setSelectedId(object.id);
    setEditing({ id: object.id, text: object.text });
  }

  function commitEdit() {
    if (!editing) return;
    if (!cancelEdit.current) session.editText(editing.id, editing.text);
    cancelEdit.current = false;
    setEditing(null);
  }

  function deleteSelected() {
    if (!selected) return;
    session.deleteObject(selected.id);
    setSelectedId(null);
    setEditing(null);
  }

  function changePage(delta: number) {
    const next = pages[pageIndex + delta];
    if (!next) return;
    setPageNumber(next.page);
    setSelectedId(null);
    setEditing(null);
  }

  function placeTextBox(event: ReactMouseEvent<HTMLButtonElement>) {
    if (!info) return;
    const rect = event.currentTarget.getBoundingClientRect();
    // A keyboard activation has no pointer position; drop the box near the page's top left.
    const keyboard = event.detail === 0;
    const x = keyboard ? 36 : (event.clientX - rect.left) / scale;
    const y = keyboard ? info.heightPt - 72 : info.heightPt - (event.clientY - rect.top) / scale;
    const id = session.addTextBox({ page: info.page, x, y, text: copy.newTextBoxText });
    setTool("select");
    if (!id) return;
    const created = session.get(id);
    if (created) startEdit(created);
  }

  async function addImageFile(file: File | undefined) {
    if (!file || !info) return;
    const image = await readImage(file);
    const widthPt = Math.min(image.width * 0.75, DEFAULT_IMAGE_WIDTH_PT, info.widthPt);
    const heightPt = widthPt * (image.height / Math.max(image.width, 1));
    const left = (info.widthPt - widthPt) / 2;
    const bottom = (info.heightPt - heightPt) / 2;
    const id = session.addImage({
      page: info.page,
      bbox: [left, bottom, left + widthPt, bottom + heightPt],
      dataUrl: image.dataUrl,
      width: image.width,
      height: image.height,
    });
    if (id) setSelectedId(id);
  }

  function onObjectKeyDown(event: KeyboardEvent<HTMLButtonElement>, object: PageObject) {
    if (event.ctrlKey || event.metaKey) {
      onShortcutKeyDown(event);
      return;
    }
    const step = event.shiftKey ? NUDGE_SHIFT_PT : NUDGE_PT;
    const arrows: Record<string, [number, number]> = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, step],
      ArrowDown: [0, -step],
    };
    const arrow = arrows[event.key];
    if (arrow) {
      event.preventDefault();
      const [dx, dy] = arrow;
      if (event.altKey) {
        const [left, bottom, right, top] = object.bbox;
        // Alt+Arrow resizes: Left/Right change the width, Up/Down the height.
        session.resizeObject(object.id, [left, bottom, right + dx, top + dy]);
      } else {
        session.moveObject(object.id, dx, dy);
      }
    } else if (event.key === "Delete" || event.key === "Backspace") {
      event.preventDefault();
      deleteSelected();
    } else if (event.key === "Escape") {
      event.preventDefault();
      setSelectedId(null);
    } else if ((event.key === "Enter" || event.key === "F2") && object.kind === "text") {
      event.preventDefault();
      startEdit(object);
    }
  }

  function onShortcutKeyDown(event: KeyboardEvent<HTMLElement>) {
    if (isTypingTarget(event.target) || !(event.ctrlKey || event.metaKey)) return;
    const key = event.key.toLowerCase();
    if (key === "z") {
      event.preventDefault();
      if (event.shiftKey) session.redo();
      else session.undo();
    } else if (key === "y") {
      event.preventDefault();
      session.redo();
    }
  }

  const tools: { id: PageTool; label: string; icon: ReactNode }[] = [
    { id: "select", label: copy.select, icon: <MousePointer2Icon aria-hidden="true" /> },
    { id: "edit-text", label: copy.editText, icon: <TypeIcon aria-hidden="true" /> },
    { id: "add-text", label: copy.addTextBox, icon: <TextCursorInputIcon aria-hidden="true" /> },
  ];

  return (
    <div data-slot="page-mode-editor" className={`flex flex-col gap-3 ${className ?? ""}`}>
      <div
        role="toolbar"
        onKeyDown={onShortcutKeyDown}
        aria-label={copy.toolbar}
        className="flex flex-wrap items-center gap-1 rounded-lg border bg-card p-1"
      >
        {tools.map((entry) => (
          <Button
            key={entry.id}
            type="button"
            variant={tool === entry.id ? "secondary" : "ghost"}
            size="sm"
            aria-pressed={tool === entry.id}
            onClick={() => setTool(entry.id)}
          >
            {entry.icon}
            {entry.label}
          </Button>
        ))}
        <Button type="button" variant="ghost" size="sm" onClick={() => fileInput.current?.click()}>
          <ImagePlusIcon aria-hidden="true" />
          {copy.addImage}
        </Button>
        <input
          ref={fileInput}
          type="file"
          accept="image/*"
          hidden
          aria-label={copy.addImage}
          data-testid="page-image-input"
          onChange={(event) => {
            const file = event.target.files?.[0];
            event.target.value = "";
            void addImageFile(file);
          }}
        />
        <Button
          type="button"
          variant="ghost"
          size="sm"
          disabled={!selected}
          onClick={deleteSelected}
        >
          <Trash2Icon aria-hidden="true" />
          {copy.deleteSelected}
        </Button>
        <span className="mx-1 h-5 w-px bg-border" aria-hidden="true" />
        <Button
          type="button"
          variant="ghost"
          size="sm"
          disabled={!session.canUndo}
          onClick={() => session.undo()}
        >
          <Undo2Icon aria-hidden="true" />
          {copy.undo}
        </Button>
        <Button
          type="button"
          variant="ghost"
          size="sm"
          disabled={!session.canRedo}
          onClick={() => session.redo()}
        >
          <Redo2Icon aria-hidden="true" />
          {copy.redo}
        </Button>
        <span className="mx-1 h-5 w-px bg-border" aria-hidden="true" />
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          aria-label={copy.previousPage}
          disabled={pageIndex <= 0}
          onClick={() => changePage(-1)}
        >
          <ChevronLeftIcon aria-hidden="true" className="rtl:rotate-180" />
        </Button>
        <span className="px-1 text-sm tabular-nums" aria-live="polite">
          {formatLabel(copy.pageOf, { page: info?.page ?? 1, count: pages.length })}
        </span>
        <Button
          type="button"
          variant="ghost"
          size="icon-sm"
          aria-label={copy.nextPage}
          disabled={pageIndex >= pages.length - 1}
          onClick={() => changePage(1)}
        >
          <ChevronRightIcon aria-hidden="true" className="rtl:rotate-180" />
        </Button>
        <Button
          type="button"
          size="sm"
          className="ms-auto"
          disabled={params.ops.length === 0 || tooMany}
          onClick={() => onApply?.(params)}
        >
          <CheckIcon aria-hidden="true" />
          {copy.applyEdits}
        </Button>
      </div>

      {tooMany && (
        <Alert variant="destructive">
          <AlertDescription>{copy.applyFailed}</AlertDescription>
        </Alert>
      )}
      {alerts.map((entry) => (
        <FontSubstitutionAlert key={entry.id} resolution={entry.resolution} labels={labels} />
      ))}

      <div className="flex flex-col gap-3 lg:flex-row">
        <div className="min-w-0 flex-1 overflow-auto rounded-lg border bg-muted p-3">
          {info && (
            <section
              // Page geometry is physical (PDF points), so the surface stays left-to-right.
              dir="ltr"
              aria-label={formatLabel(copy.pageSurface, { page: info.page })}
              data-slot="page-surface"
              className="relative mx-auto bg-white shadow-sm"
              style={{ width: info.widthPt * scale, height: info.heightPt * scale }}
            >
              <div aria-hidden="true" className="pointer-events-none absolute inset-0">
                {renderPageBackground?.(info)}
              </div>
              {tool === "add-text" && (
                <button
                  type="button"
                  aria-label={copy.addTextBox}
                  className="absolute inset-0 z-10 cursor-crosshair bg-primary/5 outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
                  onClick={placeTextBox}
                />
              )}
              {pageObjects.map((object) => {
                const box = drag?.id === object.id ? drag.current : object.bbox;
                const isSelected = object.id === selected?.id;
                const isEditing = editing?.id === object.id;
                const modified = session.isModified(object.id);
                const resolution =
                  object.kind === "text" ? session.substitutionFor(object.id) : undefined;
                const summary =
                  object.kind === "text"
                    ? object.text.slice(0, 40)
                    : object.kind === "image"
                      ? `${object.width}×${object.height}`
                      : object.id;
                const kind =
                  object.kind === "text"
                    ? copy.textRun
                    : object.kind === "image"
                      ? copy.image
                      : copy.path;
                return (
                  <div
                    key={object.id}
                    className="absolute"
                    style={{
                      left: box[0] * scale,
                      top: (info.heightPt - box[3]) * scale,
                      width: (box[2] - box[0]) * scale,
                      height: (box[3] - box[1]) * scale,
                    }}
                  >
                    <button
                      type="button"
                      data-object-id={object.id}
                      aria-label={formatLabel(copy.objectLabel, { kind, summary })}
                      aria-pressed={isSelected}
                      onClick={() => selectObject(object)}
                      onDoubleClick={() => startEdit(object)}
                      onPointerDown={(event) => startDrag(event, object, "move")}
                      onKeyDown={(event) => onObjectKeyDown(event, object)}
                      className={`absolute inset-0 overflow-hidden text-start leading-none outline-none ${
                        isSelected
                          ? "outline-2 outline-primary"
                          : "outline-1 outline-transparent hover:outline-primary/50"
                      } focus-visible:outline-2 focus-visible:outline-ring ${
                        modified ? "bg-white" : "bg-transparent"
                      }`}
                    >
                      {modified && object.kind === "text" && !isEditing && (
                        <span
                          style={{
                            color: object.color,
                            fontSize: object.fontSize * scale,
                            fontFamily: resolution?.resolvedFont ?? object.fontName,
                          }}
                        >
                          {object.text}
                        </span>
                      )}
                      {object.kind === "image" && object.dataUrl && modified && (
                        <img src={object.dataUrl} alt="" className="size-full object-fill" />
                      )}
                    </button>
                    {isEditing && editing && object.kind === "text" && (
                      <textarea
                        // biome-ignore lint/a11y/noAutofocus: the editor opens on an explicit user gesture.
                        autoFocus
                        lang="vi"
                        aria-label={copy.textEditor}
                        aria-description={copy.editHint}
                        value={editing.text}
                        onChange={(event) =>
                          setEditing({ id: editing.id, text: event.target.value.normalize("NFC") })
                        }
                        onBlur={commitEdit}
                        onKeyDown={(event) => {
                          event.stopPropagation();
                          if (event.key === "Escape") {
                            cancelEdit.current = true;
                            commitEdit();
                          } else if (event.key === "Enter" && !event.shiftKey) {
                            event.preventDefault();
                            commitEdit();
                          }
                        }}
                        className="absolute inset-0 z-20 min-h-full resize-none rounded-sm border-2 border-primary bg-white p-0 leading-none outline-none"
                        style={{
                          color: object.color,
                          fontSize: object.fontSize * scale,
                          fontFamily:
                            (draft?.substituted ? draft.resolvedFont : undefined) ??
                            object.fontName,
                        }}
                      />
                    )}
                    {isSelected && !isEditing && canResize(object) && tool === "select" && (
                      <div
                        aria-hidden="true"
                        data-slot="resize-handle"
                        className="absolute -bottom-1.5 -end-1.5 z-10 size-3 cursor-nwse-resize rounded-sm border-2 border-primary bg-background"
                        onPointerDown={(event) => startDrag(event, object, "resize")}
                      />
                    )}
                  </div>
                );
              })}
            </section>
          )}
        </div>
        <aside className="w-full shrink-0 rounded-lg border bg-card p-3 lg:w-64">
          <PageObjectInspector
            object={selected}
            liveBBox={drag && drag.id === selected?.id ? drag.current : undefined}
            labels={labels}
            onStyleChange={(patch) => selected && session.editTextStyle(selected.id, patch)}
          />
        </aside>
      </div>
    </div>
  );
}
