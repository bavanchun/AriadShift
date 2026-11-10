// The viewer adapter contract. The toolbar and workspace depend only on these types, so the
// engine (EmbedPDF today, pdf.js if the engine decision is ever reversed) is replaceable and
// the UI is testable without WebAssembly.
import type { ComponentType, ReactNode } from "react";

/** A PDF handed to the viewer. `id` identifies the document: a new id closes the old one. */
export interface PdfViewerSource {
  id: string;
  name: string;
  data: Blob | ArrayBuffer | Uint8Array;
}

export type ViewerZoomMode = "custom" | "fit-width" | "fit-page";

export interface ViewerSearchSnapshot {
  query: string;
  /** A search is scanning pages. */
  running: boolean;
  /** Pages scanned so far, from 0 to 1. */
  progress: number;
  total: number;
  /** Zero-based index of the highlighted match, or -1 when there is none. */
  current: number;
}

export interface ViewerSnapshot {
  status: "loading" | "ready" | "error";
  error?: string;
  /** One-based. */
  page: number;
  pageCount: number;
  zoomPercent: number;
  zoomMode: ViewerZoomMode;
  search: ViewerSearchSnapshot;
}

export const INITIAL_VIEWER_SNAPSHOT: ViewerSnapshot = {
  status: "loading",
  page: 1,
  pageCount: 0,
  zoomPercent: 100,
  zoomMode: "fit-width",
  search: { query: "", running: false, progress: 0, total: 0, current: -1 },
};

/** What the toolbar can ask of the engine. */
export interface ViewerCommands {
  zoomIn(): void;
  zoomOut(): void;
  setZoomMode(mode: "fit-width" | "fit-page"): void;
  /** One-based; out-of-range values are the engine's to clamp. */
  goToPage(page: number): void;
  search(query: string): void;
  nextMatch(): void;
  previousMatch(): void;
  /** Aborts a running search. */
  cancelSearch(): void;
  clearSearch(): void;
}

export interface ViewerBackendProps {
  source: PdfViewerSource;
  /** Dark-mode page inversion, applied by the engine's page layer. */
  invert: boolean;
  /** Absolute same-origin URL of the PDFium module. */
  wasmUrl: string;
  onSnapshot(snapshot: ViewerSnapshot): void;
  /** Called once the engine can take commands, and with `null` when it goes away. */
  onCommands(commands: ViewerCommands | null): void;
  className?: string;
}

export interface PagePreviewProviderProps {
  source: PdfViewerSource;
  wasmUrl: string;
  /** Receives a function that renders the preview of a one-based page. */
  children: (renderPreview: (page: number) => ReactNode) => ReactNode;
}

/** An engine: the scrolling page view with its thumbnail strip, and a page preview source. */
export interface ViewerBackend {
  Viewer: ComponentType<ViewerBackendProps>;
  PagePreviewProvider: ComponentType<PagePreviewProviderProps>;
}

export type ViewerBackendLoader = () => Promise<ViewerBackend>;

/** Strings of the viewer chrome; the host supplies translations. */
export interface ViewerLabels {
  zoomIn: string;
  zoomOut: string;
  fitWidth: string;
  fitPage: string;
  zoomLevel: (percent: number) => string;
  page: string;
  pageOf: (count: number) => string;
  search: string;
  searchPlaceholder: string;
  searchRun: string;
  searchCancel: string;
  searchClear: string;
  searchNext: string;
  searchPrevious: string;
  searching: (percent: number) => string;
  matches: (current: number, total: number) => string;
  noMatches: string;
  darkPages: string;
  loading: string;
  unavailable: string;
  documentError: string;
  toolbar: string;
}

export const DEFAULT_VIEWER_LABELS: ViewerLabels = {
  zoomIn: "Zoom in",
  zoomOut: "Zoom out",
  fitWidth: "Fit width",
  fitPage: "Fit page",
  zoomLevel: (percent) => `${percent}%`,
  page: "Page number",
  pageOf: (count) => `of ${count}`,
  search: "Search in document",
  searchPlaceholder: "Search text",
  searchRun: "Search",
  searchCancel: "Cancel search",
  searchClear: "Clear search",
  searchNext: "Next match",
  searchPrevious: "Previous match",
  searching: (percent) => `Searching… ${percent}%`,
  matches: (current, total) => `${current} of ${total}`,
  noMatches: "No matches",
  darkPages: "Dark pages",
  loading: "Opening the document on this device…",
  unavailable: "The viewer needs WebAssembly, which this browser does not provide.",
  documentError: "This document could not be displayed.",
  toolbar: "Document viewer controls",
};
