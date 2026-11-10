"use client";
// The EmbedPDF 2.15.1 headless-plugin backend. Loaded lazily by `pdf-viewer.tsx`, so the
// engine code and the PDFium module cost nothing on pages that never show a document.
//
// Integration rules from the isolation POC that this file encodes:
//  - the PDFium engine runs in a worker (a main-thread engine drops a fling to ~30 fps);
//  - `wasmUrl` is absolute and same-origin, and no font fallback is configured, so the
//    viewer makes no external request;
//  - page layers sit inside `PagePointerProvider`, or text selection silently fails.
import { createPluginRegistration } from "@embedpdf/core";
import { EmbedPDF } from "@embedpdf/core/react";
import { usePdfiumEngine } from "@embedpdf/engines/react";
import { PdfErrorCode } from "@embedpdf/models";
import {
  DocumentContent,
  DocumentManagerPluginPackage,
  useDocumentManagerCapability,
} from "@embedpdf/plugin-document-manager/react";
import {
  InteractionManagerPluginPackage,
  PagePointerProvider,
} from "@embedpdf/plugin-interaction-manager/react";
import {
  RenderLayer,
  RenderPluginPackage,
  useRenderCapability,
} from "@embedpdf/plugin-render/react";
import { Scroller, ScrollPluginPackage, useScroll } from "@embedpdf/plugin-scroll/react";
import { SearchLayer, SearchPluginPackage, useSearch } from "@embedpdf/plugin-search/react";
import { SelectionLayer, SelectionPluginPackage } from "@embedpdf/plugin-selection/react";
import { ThumbImg, ThumbnailPluginPackage, ThumbnailsPane } from "@embedpdf/plugin-thumbnail/react";
import { Viewport, ViewportPluginPackage } from "@embedpdf/plugin-viewport/react";
import { useZoom, ZoomMode, ZoomPluginPackage } from "@embedpdf/plugin-zoom/react";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { sourceToBuffer } from "./engine";
import {
  INITIAL_VIEWER_SNAPSHOT,
  type PagePreviewProviderProps,
  type PdfViewerSource,
  type ViewerBackend,
  type ViewerBackendProps,
  type ViewerCommands,
  type ViewerSnapshot,
} from "./types";

/** Reads the source into an ArrayBuffer once per document. */
function useSourceBuffer(source: PdfViewerSource): ArrayBuffer | null {
  const [loaded, setLoaded] = useState<{ id: string; buffer: ArrayBuffer } | null>(null);
  useEffect(() => {
    let current = true;
    sourceToBuffer(source).then((buffer) => {
      if (current) setLoaded({ id: source.id, buffer });
    });
    return () => {
      current = false;
    };
  }, [source]);
  return loaded?.id === source.id ? loaded.buffer : null;
}

function useDocumentPlugins(source: PdfViewerSource, buffer: ArrayBuffer | null) {
  return useMemo(
    () =>
      buffer
        ? [
            createPluginRegistration(DocumentManagerPluginPackage, {
              initialDocuments: [{ buffer, name: source.name, documentId: source.id }],
            }),
            createPluginRegistration(ViewportPluginPackage),
            createPluginRegistration(ScrollPluginPackage),
            createPluginRegistration(RenderPluginPackage),
            createPluginRegistration(InteractionManagerPluginPackage),
            createPluginRegistration(SelectionPluginPackage),
            createPluginRegistration(SearchPluginPackage),
            createPluginRegistration(ThumbnailPluginPackage),
            createPluginRegistration(ZoomPluginPackage, { defaultZoomLevel: ZoomMode.FitWidth }),
          ]
        : null,
    [source.id, source.name, buffer],
  );
}

/** Closes the document in the engine when the viewer leaves, so its pages are released. */
function useCloseOnUnmount(documentId: string) {
  const { provides } = useDocumentManagerCapability();
  const capability = useRef(provides);
  capability.current = provides;
  useEffect(
    () => () => {
      try {
        capability.current?.closeDocument(documentId);
      } catch {
        /* The registry may already be gone. */
      }
    },
    [documentId],
  );
}

const cancelled = { code: PdfErrorCode.Cancelled, message: "Search cancelled" };

/** Mirrors plugin state into the adapter snapshot and exposes the toolbar commands. */
function Bridge({
  documentId,
  onSnapshot,
  onCommands,
}: {
  documentId: string;
  onSnapshot: (snapshot: ViewerSnapshot) => void;
  onCommands: (commands: ViewerCommands | null) => void;
}) {
  useCloseOnUnmount(documentId);
  const { provides: scroll, state: scrollState } = useScroll(documentId);
  const { provides: zoom, state: zoomState } = useZoom(documentId);
  const { provides: search, state: searchState } = useSearch(documentId);
  const [progress, setProgress] = useState({ running: false, progress: 0 });
  const task = useRef<{ abort(reason: unknown): void } | null>(null);
  const scanned = useRef(0);

  useEffect(() => {
    if (!(scroll && zoom && search)) return;
    const stop = () => {
      task.current?.abort(cancelled);
      task.current = null;
      setProgress({ running: false, progress: 0 });
    };
    onCommands({
      zoomIn: () => zoom.zoomIn(),
      zoomOut: () => zoom.zoomOut(),
      setZoomMode: (mode) =>
        zoom.requestZoom(mode === "fit-width" ? ZoomMode.FitWidth : ZoomMode.FitPage),
      goToPage: (page) => scroll.scrollToPage({ pageNumber: page, behavior: "instant" }),
      search: (query) => {
        stop();
        if (!query.trim()) {
          search.stopSearch();
          return;
        }
        scanned.current = 0;
        search.startSearch();
        setProgress({ running: true, progress: 0 });
        const running = search.searchAllPages(query);
        task.current = running;
        const total = Math.max(1, scrollState.totalPages);
        running.onProgress(({ page }) => {
          if (task.current !== running) return;
          scanned.current = Math.max(scanned.current, page + 1);
          setProgress({ running: true, progress: Math.min(1, scanned.current / total) });
        });
        const finish = () => {
          if (task.current === running) {
            task.current = null;
            setProgress({ running: false, progress: 1 });
          }
        };
        running.wait(finish, finish);
      },
      nextMatch: () => search.nextResult(),
      previousMatch: () => search.previousResult(),
      cancelSearch: stop,
      clearSearch: () => {
        stop();
        search.stopSearch();
      },
    });
    return () => {
      task.current?.abort(cancelled);
      onCommands(null);
    };
  }, [scroll, zoom, search, onCommands, scrollState.totalPages]);

  useEffect(() => {
    onSnapshot({
      status: "ready",
      page: scrollState.currentPage,
      pageCount: scrollState.totalPages,
      zoomPercent: Math.round(zoomState.currentZoomLevel * 100),
      zoomMode:
        zoomState.zoomLevel === ZoomMode.FitWidth
          ? "fit-width"
          : zoomState.zoomLevel === ZoomMode.FitPage
            ? "fit-page"
            : "custom",
      search: {
        query: searchState.query,
        running: progress.running || searchState.loading,
        progress: progress.progress,
        total: searchState.total,
        current: searchState.total > 0 ? searchState.activeResultIndex : -1,
      },
    });
  }, [onSnapshot, scrollState, zoomState, searchState, progress]);
  return null;
}

function ReportError({
  onSnapshot,
  reason,
}: {
  onSnapshot: (snapshot: ViewerSnapshot) => void;
  reason: string;
}) {
  useEffect(() => {
    onSnapshot({ ...INITIAL_VIEWER_SNAPSHOT, status: "error", error: reason });
  }, [onSnapshot, reason]);
  return null;
}

/** Scrolling, virtualised page view beside a virtualised thumbnail strip. */
function Document({
  documentId,
  invert,
  onSnapshot,
  onCommands,
}: {
  documentId: string;
  invert: boolean;
  onSnapshot: ViewerBackendProps["onSnapshot"];
  onCommands: ViewerBackendProps["onCommands"];
}) {
  return (
    <DocumentContent documentId={documentId}>
      {({ isLoaded, isError }) => {
        if (isError) return <ReportError onSnapshot={onSnapshot} reason="document" />;
        if (!isLoaded) return null;
        return (
          <>
            <Bridge documentId={documentId} onSnapshot={onSnapshot} onCommands={onCommands} />
            <div
              data-slot="viewer-thumbnails"
              className="relative hidden h-full w-40 shrink-0 overflow-hidden border-e bg-muted/40 md:block"
            >
              <ThumbnailsPane documentId={documentId}>
                {(meta) => (
                  <div
                    key={meta.pageIndex}
                    data-thumb={meta.pageIndex}
                    className="absolute flex w-full flex-col items-center gap-1"
                    style={{ top: meta.top, height: meta.wrapperHeight }}
                  >
                    <div style={{ width: meta.width, height: meta.height }}>
                      <ThumbImg documentId={documentId} meta={meta} />
                    </div>
                    <span className="text-xs text-muted-foreground tabular-nums">
                      {meta.pageIndex + 1}
                    </span>
                  </div>
                )}
              </ThumbnailsPane>
            </div>
            <Viewport documentId={documentId} className="min-w-0 flex-1 bg-muted">
              <Scroller
                documentId={documentId}
                renderPage={({ width, height, pageIndex }) => (
                  <div
                    data-page={pageIndex}
                    className="relative bg-white shadow-elevation-1"
                    style={{ width, height }}
                  >
                    <PagePointerProvider documentId={documentId} pageIndex={pageIndex}>
                      <div
                        className="absolute inset-0"
                        style={invert ? { filter: "invert(1) hue-rotate(180deg)" } : undefined}
                      >
                        <RenderLayer documentId={documentId} pageIndex={pageIndex} />
                      </div>
                      <SelectionLayer documentId={documentId} pageIndex={pageIndex} />
                      <SearchLayer documentId={documentId} pageIndex={pageIndex} />
                    </PagePointerProvider>
                  </div>
                )}
              />
            </Viewport>
          </>
        );
      }}
    </DocumentContent>
  );
}

/** Starts the engine and the document, reporting engine failures through the snapshot. */
function useEngineSession(
  source: PdfViewerSource,
  wasmUrl: string,
  onSnapshot: (snapshot: ViewerSnapshot) => void,
) {
  const { engine, error } = usePdfiumEngine({ wasmUrl, worker: true, fontFallback: null });
  const buffer = useSourceBuffer(source);
  const plugins = useDocumentPlugins(source, buffer);
  useEffect(() => {
    if (error)
      onSnapshot({ ...INITIAL_VIEWER_SNAPSHOT, status: "error", error: String(error.message) });
  }, [error, onSnapshot]);
  return { engine, plugins };
}

export function EmbedPdfViewer({
  source,
  invert,
  wasmUrl,
  onSnapshot,
  onCommands,
  className,
}: ViewerBackendProps) {
  const { engine, plugins } = useEngineSession(source, wasmUrl, onSnapshot);
  if (!(engine && plugins)) return null;
  return (
    <div className={className ?? "flex h-full min-h-0 w-full"} data-slot="pdf-viewer-surface">
      <EmbedPDF key={source.id} engine={engine} plugins={plugins}>
        {({ activeDocumentId }) =>
          activeDocumentId ? (
            <Document
              documentId={activeDocumentId}
              invert={invert}
              onSnapshot={onSnapshot}
              onCommands={onCommands}
            />
          ) : null
        }
      </EmbedPDF>
    </div>
  );
}

/** Renders a page to an object URL while it is near the viewport, and revokes it after. */
function PagePreview({ documentId, page }: { documentId: string; page: number }) {
  const { provides } = useRenderCapability();
  const host = useRef<HTMLDivElement>(null);
  const [visible, setVisible] = useState(false);
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    const node = host.current;
    if (!node || typeof IntersectionObserver === "undefined") {
      setVisible(true);
      return;
    }
    const observer = new IntersectionObserver(
      (entries) => setVisible(entries.some((entry) => entry.isIntersecting)),
      { rootMargin: "300px" },
    );
    observer.observe(node);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    if (!(visible && provides)) return;
    const task = provides
      .forDocument(documentId)
      .renderPage({ pageIndex: page - 1, options: { scaleFactor: 0.3 } });
    let created: string | null = null;
    task.wait(
      (blob) => {
        created = URL.createObjectURL(blob);
        setUrl(created);
      },
      () => undefined,
    );
    return () => {
      task.abort(cancelled);
      if (created) URL.revokeObjectURL(created);
      setUrl(null);
    };
  }, [visible, provides, documentId, page]);
  return (
    <div ref={host} className="size-full">
      {url && <img src={url} alt="" className="size-full object-contain" draggable={false} />}
    </div>
  );
}

/** Mounts a document so a grid of page previews can render from it. */
export function EmbedPdfPagePreviewProvider({
  source,
  wasmUrl,
  children,
}: PagePreviewProviderProps) {
  const { engine, plugins } = useEngineSession(source, wasmUrl, () => undefined);
  if (!(engine && plugins)) return <>{children(() => null)}</>;
  return (
    <EmbedPDF key={source.id} engine={engine} plugins={plugins}>
      {({ activeDocumentId }): ReactNode =>
        activeDocumentId ? (
          <DocumentContent documentId={activeDocumentId}>
            {({ isLoaded }) =>
              children((page) =>
                isLoaded ? <PagePreview documentId={activeDocumentId} page={page} /> : null,
              )
            }
          </DocumentContent>
        ) : (
          children(() => null)
        )
      }
    </EmbedPDF>
  );
}

const backend: ViewerBackend = {
  Viewer: EmbedPdfViewer,
  PagePreviewProvider: EmbedPdfPagePreviewProvider,
};

export default backend;
