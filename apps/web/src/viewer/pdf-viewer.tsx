"use client";

import { Button } from "@ariadshift/ui/components/button";
import { Progress } from "@ariadshift/ui/components/progress";
import { cn } from "@ariadshift/ui/lib/utils";
import {
  ChevronDownIcon,
  ChevronUpIcon,
  MaximizeIcon,
  MoonIcon,
  MoveHorizontalIcon,
  SearchIcon,
  XIcon,
  ZoomInIcon,
  ZoomOutIcon,
} from "lucide-react";
import { type FormEvent, useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { resolveWasmUrl, webAssemblySupported } from "./engine";
import {
  DEFAULT_VIEWER_LABELS,
  INITIAL_VIEWER_SNAPSHOT,
  type PdfViewerSource,
  type ViewerBackend,
  type ViewerBackendLoader,
  type ViewerCommands,
  type ViewerLabels,
  type ViewerSnapshot,
} from "./types";

/** Loads the EmbedPDF backend on first use; it is a separate chunk. */
export const loadEmbedPdfBackend: ViewerBackendLoader = async () =>
  (await import("./embedpdf-backend")).default;

export interface PdfViewerProps {
  source: PdfViewerSource;
  /** Supplies the engine. Defaults to EmbedPDF; tests pass a stub. */
  loadBackend?: ViewerBackendLoader;
  /** Overrides the PDFium URL; it must be absolute and same-origin. */
  wasmUrl?: string;
  labels?: Partial<ViewerLabels>;
  /** One-based page to scroll to once the document is ready. */
  initialPage?: number;
  /** Scrolls to this page whenever `nonce` changes, for example to follow an AI citation. */
  pageRequest?: { page: number; nonce: number };
  /** Reports the page in view, so another mode can open on it. */
  onPageChange?: (page: number) => void;
  className?: string;
}

export type BackendState =
  | { status: "loading" }
  | { status: "ready"; backend: ViewerBackend }
  | { status: "unavailable" };

/** Loads the engine once per loader; the workspace shares it between viewer and page grid. */
export function useViewerBackend(loader: ViewerBackendLoader): BackendState {
  const [state, setState] = useState<BackendState>(() =>
    webAssemblySupported() ? { status: "loading" } : { status: "unavailable" },
  );
  useEffect(() => {
    if (!webAssemblySupported()) return;
    let current = true;
    loader().then(
      (backend) => current && setState({ status: "ready", backend }),
      () => current && setState({ status: "unavailable" }),
    );
    return () => {
      current = false;
    };
  }, [loader]);
  return state;
}

/** A PDF viewer: zoom, page jump, text search, dark pages and thumbnails over any backend. */
export function PdfViewer(props: PdfViewerProps) {
  // The dark-pages choice belongs to the reader, so it survives opening another document.
  const [invert, setInvert] = useState(false);
  // A new document id remounts the session, dropping the previous document's state and
  // engine instance together; resetting in an effect would race the new engine's own.
  return (
    <ViewerSession key={props.source.id} {...props} invert={invert} onInvertChange={setInvert} />
  );
}

function ViewerSession({
  source,
  loadBackend = loadEmbedPdfBackend,
  wasmUrl,
  labels: overrides,
  initialPage,
  pageRequest,
  onPageChange,
  className,
  invert,
  onInvertChange,
}: PdfViewerProps & { invert: boolean; onInvertChange: (invert: boolean) => void }) {
  const labels = useMemo(() => ({ ...DEFAULT_VIEWER_LABELS, ...overrides }), [overrides]);
  const backend = useViewerBackend(loadBackend);
  const [snapshot, setSnapshot] = useState<ViewerSnapshot>(INITIAL_VIEWER_SNAPSHOT);
  const [commands, setCommands] = useState<ViewerCommands | null>(null);
  const url = useMemo(() => {
    try {
      return wasmUrl ?? resolveWasmUrl();
    } catch {
      return null;
    }
  }, [wasmUrl]);

  const onSnapshot = useCallback((next: ViewerSnapshot) => setSnapshot(next), []);
  const onCommands = useCallback((next: ViewerCommands | null) => setCommands(next), []);

  // Scroll to a requested page (at open, or when the host asks again), then report page changes.
  // Reports wait until the jump lands, so the page the host asked for is not overwritten by 1.
  const request = useMemo(
    () =>
      pageRequest ??
      (initialPage !== undefined && initialPage > 1 ? { page: initialPage, nonce: 0 } : null),
    [pageRequest, initialPage],
  );
  const handled = useRef<number | null>(null);
  const [landed, setLanded] = useState<number | null>(null);
  const ready = snapshot.status === "ready" && commands !== null;
  const goal = request ? Math.min(request.page, Math.max(snapshot.pageCount, 1)) : null;
  useEffect(() => {
    if (!(ready && request) || handled.current === request.nonce) return;
    handled.current = request.nonce;
    setLanded(null);
    commands?.goToPage(Math.min(request.page, Math.max(snapshot.pageCount, 1)));
  }, [ready, request, commands, snapshot.pageCount]);
  useEffect(() => {
    if (ready && request && handled.current === request.nonce && snapshot.page === goal) {
      setLanded(request.nonce);
    }
  }, [ready, request, goal, snapshot.page]);
  const waiting = request !== null && landed !== request.nonce;
  const reportedPage = useRef(onPageChange);
  reportedPage.current = onPageChange;
  // While a page is laid out the engine can briefly settle on a page nobody scrolled to (a
  // fit-width zoom measured before the pane has its width shows every page at once). That is not
  // the reader's position, so a page is reported only once the reader has moved or the host has
  // asked for a page and the jump has landed.
  const surface = useRef<HTMLElement>(null);
  const [moved, setMoved] = useState(false);
  useEffect(() => {
    const node = surface.current;
    if (!node) return;
    const markMoved = () => setMoved(true);
    const events = ["pointerdown", "wheel", "keydown", "touchstart"] as const;
    for (const name of events) node.addEventListener(name, markMoved, { capture: true });
    return () => {
      for (const name of events) node.removeEventListener(name, markMoved, { capture: true });
    };
  }, []);
  const reporting = moved || request !== null;
  useEffect(() => {
    if (snapshot.status === "ready" && !waiting && reporting) reportedPage.current?.(snapshot.page);
  }, [snapshot.status, snapshot.page, waiting, reporting]);

  const failed = backend.status === "unavailable" || snapshot.status === "error" || !url;
  return (
    <section
      ref={surface}
      data-slot="pdf-viewer"
      aria-label={source.name}
      className={cn("flex min-h-0 flex-col overflow-hidden rounded-xl border bg-card", className)}
    >
      <ViewerToolbar
        snapshot={snapshot}
        commands={commands}
        labels={labels}
        invert={invert}
        onInvertChange={onInvertChange}
      />
      <div className="relative flex min-h-0 flex-1">
        {failed ? (
          <p role="alert" className="m-auto p-6 text-center text-sm text-muted-foreground">
            {backend.status === "unavailable" || !url ? labels.unavailable : labels.documentError}
          </p>
        ) : (
          <>
            {backend.status === "ready" && url && (
              <backend.backend.Viewer
                source={source}
                invert={invert}
                wasmUrl={url}
                onSnapshot={onSnapshot}
                onCommands={onCommands}
              />
            )}
            {snapshot.status === "loading" && (
              <p
                role="status"
                className="absolute inset-0 m-auto h-fit text-center text-sm text-muted-foreground"
              >
                {labels.loading}
              </p>
            )}
          </>
        )}
      </div>
    </section>
  );
}

export interface ViewerToolbarProps {
  snapshot: ViewerSnapshot;
  commands: ViewerCommands | null;
  labels?: ViewerLabels;
  invert: boolean;
  onInvertChange: (invert: boolean) => void;
}

const iconButton = "size-8";

/** The viewer controls. Disabled until the engine reports a ready document. */
export function ViewerToolbar({
  snapshot,
  commands,
  labels = DEFAULT_VIEWER_LABELS,
  invert,
  onInvertChange,
}: ViewerToolbarProps) {
  const ready = snapshot.status === "ready" && commands !== null;
  const pageId = useId();
  const searchId = useId();
  const [pageDraft, setPageDraft] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const { search } = snapshot;

  function jump(event: FormEvent) {
    event.preventDefault();
    const value = Number.parseInt(pageDraft ?? "", 10);
    setPageDraft(null);
    if (Number.isFinite(value) && snapshot.pageCount > 0)
      commands?.goToPage(Math.min(Math.max(value, 1), snapshot.pageCount));
  }
  function runSearch(event: FormEvent) {
    event.preventDefault();
    commands?.search(query);
  }

  return (
    <div
      role="toolbar"
      aria-label={labels.toolbar}
      data-slot="viewer-toolbar"
      className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b bg-card px-3 py-2"
    >
      <div className="flex items-center gap-1">
        <Button
          variant="ghost"
          size="icon"
          className={iconButton}
          aria-label={labels.zoomOut}
          disabled={!ready}
          onClick={() => commands?.zoomOut()}
        >
          <ZoomOutIcon aria-hidden="true" />
        </Button>
        <output
          aria-label={labels.zoomLevel(snapshot.zoomPercent)}
          className="min-w-12 text-center text-sm tabular-nums"
        >
          {labels.zoomLevel(snapshot.zoomPercent)}
        </output>
        <Button
          variant="ghost"
          size="icon"
          className={iconButton}
          aria-label={labels.zoomIn}
          disabled={!ready}
          onClick={() => commands?.zoomIn()}
        >
          <ZoomInIcon aria-hidden="true" />
        </Button>
        <Button
          variant={snapshot.zoomMode === "fit-width" ? "secondary" : "ghost"}
          size="icon"
          className={iconButton}
          aria-label={labels.fitWidth}
          aria-pressed={snapshot.zoomMode === "fit-width"}
          disabled={!ready}
          onClick={() => commands?.setZoomMode("fit-width")}
        >
          <MoveHorizontalIcon aria-hidden="true" />
        </Button>
        <Button
          variant={snapshot.zoomMode === "fit-page" ? "secondary" : "ghost"}
          size="icon"
          className={iconButton}
          aria-label={labels.fitPage}
          aria-pressed={snapshot.zoomMode === "fit-page"}
          disabled={!ready}
          onClick={() => commands?.setZoomMode("fit-page")}
        >
          <MaximizeIcon aria-hidden="true" />
        </Button>
      </div>

      <form onSubmit={jump} className="flex items-center gap-2 text-sm">
        <label htmlFor={pageId} className="sr-only">
          {labels.page}
        </label>
        <input
          id={pageId}
          type="number"
          inputMode="numeric"
          min={1}
          max={Math.max(snapshot.pageCount, 1)}
          disabled={!ready}
          value={pageDraft ?? String(snapshot.page)}
          onChange={(event) => setPageDraft(event.target.value)}
          onBlur={() => setPageDraft(null)}
          className="h-8 w-16 rounded-lg border bg-background px-2 text-center tabular-nums outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
        />
        <span className="text-muted-foreground tabular-nums">
          {labels.pageOf(snapshot.pageCount)}
        </span>
      </form>

      <search className="flex min-w-0">
        <form onSubmit={runSearch} className="flex min-w-0 items-center gap-1">
          <label htmlFor={searchId} className="sr-only">
            {labels.search}
          </label>
          <input
            id={searchId}
            type="search"
            value={query}
            disabled={!ready}
            placeholder={labels.searchPlaceholder}
            onChange={(event) => setQuery(event.target.value)}
            className="h-8 w-44 min-w-0 rounded-lg border bg-background px-2 text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
          />
          <Button
            type="submit"
            variant="ghost"
            size="icon"
            className={iconButton}
            aria-label={labels.searchRun}
            disabled={!ready || query.trim() === ""}
          >
            <SearchIcon aria-hidden="true" />
          </Button>
          {search.running ? (
            <>
              <Progress
                value={Math.round(search.progress * 100)}
                aria-label={labels.searching(Math.round(search.progress * 100))}
                className="w-20"
              />
              <span role="status" className="text-xs whitespace-nowrap text-muted-foreground">
                {labels.searching(Math.round(search.progress * 100))}
              </span>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                className={iconButton}
                aria-label={labels.searchCancel}
                onClick={() => commands?.cancelSearch()}
              >
                <XIcon aria-hidden="true" />
              </Button>
            </>
          ) : (
            search.query !== "" && (
              <>
                <span role="status" className="text-xs whitespace-nowrap text-muted-foreground">
                  {search.total > 0
                    ? labels.matches(search.current + 1, search.total)
                    : labels.noMatches}
                </span>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className={iconButton}
                  aria-label={labels.searchPrevious}
                  disabled={search.total === 0}
                  onClick={() => commands?.previousMatch()}
                >
                  <ChevronUpIcon aria-hidden="true" />
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className={iconButton}
                  aria-label={labels.searchNext}
                  disabled={search.total === 0}
                  onClick={() => commands?.nextMatch()}
                >
                  <ChevronDownIcon aria-hidden="true" />
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon"
                  className={iconButton}
                  aria-label={labels.searchClear}
                  onClick={() => {
                    setQuery("");
                    commands?.clearSearch();
                  }}
                >
                  <XIcon aria-hidden="true" />
                </Button>
              </>
            )
          )}
        </form>
      </search>

      <Button
        variant={invert ? "secondary" : "ghost"}
        size="sm"
        className="ms-auto"
        aria-pressed={invert}
        onClick={() => onInvertChange(!invert)}
      >
        <MoonIcon aria-hidden="true" />
        {labels.darkPages}
      </Button>
    </div>
  );
}
