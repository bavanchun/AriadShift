"use client";

import { Button } from "@ariadshift/ui/components/button";
import { ChevronLeftIcon, ChevronRightIcon } from "lucide-react";
import {
  type KeyboardEvent,
  type ReactNode,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import { useStore } from "zustand";
import { AnnotationCanvas, type PageSize } from "./annotation-canvas";
import { AnnotationList } from "./annotation-list";
import {
  type AnnotationStore,
  createAnnotationStore,
  draftFromGesture,
  type ExistingAnnotation,
  type Point,
  pendingCount,
  planAnnotationCalls,
} from "./annotation-store";
import { AnnotationToolbar, handleAnnotationShortcut } from "./annotation-toolbar";
import { useAnnotateCopy } from "./copy";
import { FormDesignerView, FormDesignOverlay } from "./form-designer-view";
import { FormFieldOverlay, FormFillerView } from "./form-filler-view";
import {
  createFormDesignStore,
  createFormFillStore,
  designIssues,
  type FormDesignStore,
  type FormFieldInfo,
  type FormFillStore,
  hasFillChanges,
  planFormCreateCalls,
  planFormFillCalls,
} from "./form-model";
import type { EditorToolCall } from "./tool-params";

export type SurfaceMode = "annotate" | "fill" | "design";
const MODES: readonly SurfaceMode[] = ["annotate", "fill", "design"];

const NO_FIELDS: readonly FormFieldInfo[] = [];
const NO_ANNOTATIONS: readonly ExistingAnnotation[] = [];

/** Half-length of the default freehand stroke placed from the keyboard, in points. */
const KEYBOARD_STROKE = 30;

export interface AnnotateFormsSurfaceProps {
  pageCount: number;
  /** Size of the pages in PDF points; the drawing layer and overlays scale to it. */
  pageSize: PageSize;
  /** Renders the page view underneath the drawing layer. */
  renderPage?: (page: number) => ReactNode;
  /** Fields read from the PDF with `pdf.forms_list`. */
  formFields?: readonly FormFieldInfo[];
  /** Annotations already stored in the PDF, which can be marked for deletion. Re-read them after an apply. */
  existingAnnotations?: readonly ExistingAnnotation[];
  /** One-based page to open on. */
  initialPage?: number;
  /** Reports the page in view, so another mode can open on it. */
  onPageChange?: (page: number) => void;
  mode?: SurfaceMode;
  onModeChange?: (mode: SurfaceMode) => void;
  /** Runs the planned `pdf.*` calls, in order, through the tool runner. */
  onApply: (calls: EditorToolCall[]) => void | Promise<void>;
  busy?: boolean;
  /** Supplied stores let a parent observe or reset the surface. */
  annotationStore?: AnnotationStore;
  fillStore?: FormFillStore;
  designStore?: FormDesignStore;
}

/** Annotate, fill and design forms on one surface, switching between the three sub-modes. */
export function AnnotateFormsSurface({
  pageCount,
  pageSize,
  renderPage,
  formFields = NO_FIELDS,
  existingAnnotations = NO_ANNOTATIONS,
  initialPage,
  onPageChange,
  mode: controlledMode,
  onModeChange,
  onApply,
  busy = false,
  annotationStore,
  fillStore,
  designStore,
}: AnnotateFormsSurfaceProps) {
  const copy = useAnnotateCopy();
  const idPrefix = useId();
  const [ownMode, setOwnMode] = useState<SurfaceMode>("annotate");
  const mode = controlledMode ?? ownMode;
  const [page, setPage] = useState(initialPage ?? 1);
  const [error, setError] = useState<string | null>(null);
  const [applying, setApplying] = useState(false);

  const annotations = useMemo(() => annotationStore ?? createAnnotationStore(), [annotationStore]);
  const filling = useMemo(
    () => fillStore ?? createFormFillStore(formFields),
    [fillStore, formFields],
  );
  const designing = useMemo(() => designStore ?? createFormDesignStore(), [designStore]);
  const existingKey = useMemo(() => JSON.stringify(existingAnnotations), [existingAnnotations]);
  useEffect(() => {
    annotations.getState().setExisting(JSON.parse(existingKey) as ExistingAnnotation[]);
  }, [annotations, existingKey]);

  const annotationState = useStore(annotations);
  const fillState = useStore(filling);
  const designState = useStore(designing);
  const current = Math.min(Math.max(page, 1), Math.max(pageCount, 1));
  const reportPage = useRef(onPageChange);
  reportPage.current = onPageChange;
  useEffect(() => {
    reportPage.current?.(current);
  }, [current]);

  function switchMode(next: SurfaceMode) {
    setOwnMode(next);
    onModeChange?.(next);
  }

  function placeAtCentre() {
    const centre: Point = [pageSize.width / 2, pageSize.height / 2];
    const { settings } = annotations.getState();
    const draft = draftFromGesture(
      { ...settings, page: current },
      settings.tool === "ink"
        ? {
            type: "stroke",
            points: [
              [centre[0] - KEYBOARD_STROKE, centre[1]],
              [centre[0], centre[1] + KEYBOARD_STROKE / 3],
              [centre[0] + KEYBOARD_STROKE, centre[1]],
            ],
          }
        : { type: "drag", from: centre, to: centre },
    );
    if (draft) annotations.getState().add(draft);
  }

  const plan = (): EditorToolCall[] =>
    mode === "annotate"
      ? planAnnotationCalls(annotationState)
      : mode === "fill"
        ? planFormFillCalls(fillState)
        : planFormCreateCalls(designState.fields);

  const canApply =
    !busy &&
    !applying &&
    (mode === "annotate"
      ? pendingCount(annotationState) > 0
      : mode === "fill"
        ? hasFillChanges(fillState) || fillState.flatten
        : designState.fields.length > 0 && designIssues(designState.fields).size === 0);

  async function apply() {
    setError(null);
    setApplying(true);
    try {
      await onApply(plan());
      // Applied annotations now live in the PDF; the caller re-reads them into `existingAnnotations`.
      if (mode === "annotate") annotations.getState().reset();
      else if (mode === "fill") filling.getState().reset();
      else designing.getState().reset();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : copy.surface.failed);
    } finally {
      setApplying(false);
    }
  }

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (mode === "annotate") handleAnnotationShortcut(event, annotations);
  }

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: shortcuts bubble from focused controls
    <div
      data-slot="annotate-forms-surface"
      data-mode={mode}
      className="flex min-w-0 flex-col gap-3"
      onKeyDown={onKeyDown}
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <fieldset
          aria-label={copy.surface.modes}
          className="m-0 min-w-0 border-0 p-0 flex items-center gap-1.5"
        >
          {MODES.map((entry) => (
            <Button
              key={entry}
              type="button"
              size="sm"
              variant={mode === entry ? "default" : "outline"}
              aria-pressed={mode === entry}
              onClick={() => switchMode(entry)}
            >
              {copy.surface[entry]}
            </Button>
          ))}
        </fieldset>
        <fieldset
          aria-label={copy.surface.page}
          className="m-0 min-w-0 border-0 p-0 flex items-center gap-1.5"
        >
          <Button
            type="button"
            size="icon-sm"
            variant="outline"
            aria-label={copy.surface.previousPage}
            disabled={current <= 1}
            onClick={() => setPage(current - 1)}
          >
            <ChevronLeftIcon aria-hidden="true" className="rtl:rotate-180" />
          </Button>
          <span aria-live="polite" className="text-sm tabular-nums">
            {copy.surface.pageOf(current, pageCount)}
          </span>
          <Button
            type="button"
            size="icon-sm"
            variant="outline"
            aria-label={copy.surface.nextPage}
            disabled={current >= pageCount}
            onClick={() => setPage(current + 1)}
          >
            <ChevronRightIcon aria-hidden="true" className="rtl:rotate-180" />
          </Button>
        </fieldset>
      </div>

      {mode === "annotate" ? (
        <AnnotationToolbar store={annotations} onPlaceAtCentre={placeAtCentre} />
      ) : null}

      <div className="grid min-w-0 gap-4 lg:grid-cols-[minmax(0,1fr)_20rem]">
        <div dir="ltr" className="min-w-0">
          {mode === "annotate" ? (
            <AnnotationCanvas store={annotations} page={current} size={pageSize}>
              {renderPage?.(current)}
            </AnnotationCanvas>
          ) : (
            <div
              className="relative w-full overflow-hidden rounded-lg border bg-card"
              style={{ aspectRatio: `${pageSize.width} / ${pageSize.height}` }}
            >
              {renderPage?.(current)}
              {mode === "fill" ? (
                <FormFieldOverlay
                  store={filling}
                  page={current}
                  size={pageSize}
                  idPrefix={idPrefix}
                />
              ) : (
                <FormDesignOverlay store={designing} page={current} size={pageSize} />
              )}
            </div>
          )}
        </div>

        <div className="flex min-w-0 flex-col gap-3">
          {mode === "annotate" ? <AnnotationList store={annotations} /> : null}
          {mode === "fill" ? <FormFillerView store={filling} idPrefix={idPrefix} /> : null}
          {mode === "design" ? (
            <FormDesignerView
              store={designing}
              page={current}
              pageCount={pageCount}
              pageSize={pageSize}
            />
          ) : null}
          {error ? (
            <p role="alert" className="text-sm text-destructive">
              {error}
            </p>
          ) : null}
          <Button type="button" disabled={!canApply} onClick={apply}>
            {applying
              ? copy.surface.applying
              : mode === "design"
                ? copy.design.create
                : copy.surface.apply}
          </Button>
        </div>
      </div>
    </div>
  );
}
