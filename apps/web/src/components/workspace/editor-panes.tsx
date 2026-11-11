"use client";
// The center column's editor modes: the switch, and one pane each for Annotate, Document and
// Page. A pane reads what it needs from the workspace's editor hook and hands edits back to the
// workspace's tool runner, so no pane carries conversion or document logic of its own.
import { Button } from "@ariadshift/ui/components/button";
import { type ReactNode, useMemo, useState } from "react";
import { AnnotateFormsSurface, type EditorToolCall } from "@/editor/annotate";
import { DocumentModeEditor, type ExportRequest } from "@/editor/document";
import {
  type EditApplyParams,
  type PageEditSession,
  PageModeEditor,
  type PageObjectsModel,
} from "@/editor/page";
import type { EditorVersionStore } from "@/editor/storage";
import { resolveWasmUrl } from "@/viewer/engine";
import type { useViewerBackend } from "@/viewer/pdf-viewer";
import type { PdfViewerSource } from "@/viewer/types";
import { useWorkspaceCopy } from "./copy";
import {
  type ActiveDocumentData,
  type EditorLocationState,
  initialPointFor,
  locationFromSelection,
} from "./editor-data";
import type { IrBundle, Loaded, WorkspaceMode } from "./use-editor-workspace";

const MODES: WorkspaceMode[] = ["view", "annotate", "document", "page"];

/** The View · Annotate · Document · Page switch. */
export function WorkspaceModeSwitch({
  mode,
  onChange,
}: {
  mode: WorkspaceMode;
  onChange: (mode: WorkspaceMode) => void;
}) {
  const copy = useWorkspaceCopy().editor;
  return (
    <fieldset
      data-testid="workspace-mode-switch"
      aria-label={copy.modeSwitch}
      className="m-0 flex min-w-0 items-center gap-1.5 border-0 p-0"
    >
      {MODES.map((entry) => (
        <Button
          key={entry}
          type="button"
          size="sm"
          variant={mode === entry ? "default" : "outline"}
          aria-pressed={mode === entry}
          data-testid={`workspace-mode-${entry}`}
          onClick={() => onChange(entry)}
        >
          {copy[entry]}
        </Button>
      ))}
    </fieldset>
  );
}

/** Shows the loading and failure states of something a mode reads before it can open. */
function LoadGate<T>({
  entry,
  retry,
  children,
}: {
  entry: Loaded<T> | undefined;
  retry: () => void;
  children: (value: T) => ReactNode;
}) {
  const copy = useWorkspaceCopy().editor;
  if (!entry || entry.status === "loading") {
    return (
      <p role="status" className="p-6 text-center text-sm text-muted-foreground">
        {copy.preparing}
      </p>
    );
  }
  if (entry.status === "failed") {
    return (
      <div role="alert" className="flex flex-col items-start gap-2 p-6 text-sm">
        <p>{copy.failed(entry.message)}</p>
        <Button type="button" variant="outline" size="sm" onClick={retry}>
          {copy.retry}
        </Button>
      </div>
    );
  }
  return <>{children(entry.value)}</>;
}

type Backend = ReturnType<typeof useViewerBackend>;

/** Renders PDFium page previews for the panes that draw a page under their own layer. */
function WithPagePreviews({
  source,
  backend,
  children,
}: {
  source: PdfViewerSource;
  backend: Backend;
  children: (renderPreview: ((page: number) => ReactNode) | undefined) => ReactNode;
}) {
  const wasmUrl = useMemo(() => {
    try {
      return resolveWasmUrl();
    } catch {
      return null;
    }
  }, []);
  if (backend.status !== "ready" || !wasmUrl) return <>{children(undefined)}</>;
  const { PagePreviewProvider } = backend.backend;
  return (
    <PagePreviewProvider source={source} wasmUrl={wasmUrl}>
      {(renderPreview) => children(renderPreview)}
    </PagePreviewProvider>
  );
}

export interface DocumentPaneProps {
  docKey: string;
  locale?: string;
  ir: { entry: Loaded<IrBundle> | undefined; retry: () => void };
  store: EditorVersionStore | null;
  redacted: boolean;
  location: EditorLocationState;
  onLocationChange: (location: EditorLocationState) => void;
  onExport: (request: ExportRequest) => Promise<void>;
}

/** Document mode: reflow editing of the PDF's IR, with version history and fidelity-checked export. */
export function DocumentPane({
  docKey,
  locale,
  ir,
  store,
  redacted,
  location,
  onLocationChange,
  onExport,
}: DocumentPaneProps) {
  // Where the editor opens is where the user was when they switched; it is not followed afterwards.
  const [initialPoint] = useState(() => initialPointFor(location));
  return (
    <LoadGate entry={ir.entry} retry={ir.retry}>
      {({ session }) =>
        store ? (
          <DocumentModeEditor
            key={docKey}
            docId={docKey}
            session={session}
            store={store}
            redacted={redacted}
            locale={locale}
            initialPoint={initialPoint}
            onSelectionLocationChange={(selection) =>
              onLocationChange(locationFromSelection(selection, location))
            }
            onExport={onExport}
            className="h-[70vh] overflow-hidden rounded-xl border"
          />
        ) : null
      }
    </LoadGate>
  );
}

export interface PagePaneProps {
  docKey: string;
  source: PdfViewerSource;
  backend: Backend;
  /** The `pdf.info` read; its page sizes stand in for a document whose objects carry no layout. */
  data: { entry: Loaded<ActiveDocumentData> | undefined; retry: () => void };
  objects: { entry: Loaded<PageObjectsModel> | undefined; retry: () => void };
  /** The file's edit session, kept by the workspace so edits outlive this pane. */
  pageSession: (model: PageObjectsModel) => PageEditSession;
  location: EditorLocationState;
  onLocationChange: (location: EditorLocationState) => void;
  onApply: (params: EditApplyParams) => void | Promise<void>;
}

/** The largest width, in screen pixels, a page is drawn at in the center column. */
const PAGE_WIDTH_PX = 520;

/** Page mode: edit the text, images and paths of one page in place. */
export function PagePane({
  docKey,
  source,
  backend,
  data,
  objects,
  pageSession,
  location,
  onLocationChange,
  onApply,
}: PagePaneProps) {
  const copy = useWorkspaceCopy().editor;
  const [initialPage] = useState(() => location.page);
  return (
    <LoadGate entry={objects.entry} retry={objects.retry}>
      {(model) => {
        const fallback = data.entry;
        const pages =
          model.pages.length > 0
            ? model.pages
            : fallback?.status === "ready"
              ? [...fallback.value.pages]
              : [];
        const first = pages[0];
        if (!first) {
          // The sizes come from the same read as Annotate, so a failure reads and retries the same way.
          if (fallback?.status === "failed") {
            return (
              <div role="alert" className="flex flex-col items-start gap-2 p-6 text-sm">
                <p>{copy.failed(fallback.message)}</p>
                <Button type="button" variant="outline" size="sm" onClick={data.retry}>
                  {copy.retry}
                </Button>
              </div>
            );
          }
          return (
            <p role="status" className="p-6 text-center text-sm text-muted-foreground">
              {copy.preparing}
            </p>
          );
        }
        const scale = Math.min(1, PAGE_WIDTH_PX / first.widthPt);
        return (
          <div className="flex flex-col gap-2">
            {model.source === "ir" && (
              <p
                role="note"
                data-testid="page-objects-estimated"
                className="text-xs text-muted-foreground"
              >
                {copy.pageEstimated}
              </p>
            )}
            <WithPagePreviews source={source} backend={backend}>
              {(renderPreview) => (
                <PageModeEditor
                  key={docKey}
                  pages={pages}
                  objects={model.objects}
                  session={pageSession(model)}
                  initialPage={Math.min(initialPage, pages.length)}
                  scale={scale}
                  onApply={onApply}
                  onLocationChange={(next) => onLocationChange({ ...next })}
                  renderPageBackground={
                    renderPreview ? (page) => renderPreview(page.page) : undefined
                  }
                />
              )}
            </WithPagePreviews>
          </div>
        );
      }}
    </LoadGate>
  );
}

export interface AnnotatePaneProps {
  docKey: string;
  source: PdfViewerSource;
  backend: Backend;
  data: { entry: Loaded<ActiveDocumentData> | undefined; retry: () => void };
  busy: boolean;
  location: EditorLocationState;
  onLocationChange: (location: EditorLocationState) => void;
  onApply: (calls: EditorToolCall[]) => void | Promise<void>;
}

/** Annotate mode: annotations, form filling and the form designer, run through the tool runner. */
export function AnnotatePane({
  docKey,
  source,
  backend,
  data,
  busy,
  location,
  onLocationChange,
  onApply,
}: AnnotatePaneProps) {
  const [initialPage] = useState(() => location.page);
  return (
    <LoadGate entry={data.entry} retry={data.retry}>
      {(value) => {
        const first = value.pages[0];
        if (!first) return null;
        return (
          <WithPagePreviews source={source} backend={backend}>
            {(renderPreview) => (
              <AnnotateFormsSurface
                key={docKey}
                pageCount={value.pages.length}
                pageSize={{ width: first.widthPt, height: first.heightPt }}
                renderPage={renderPreview}
                formFields={value.fields}
                existingAnnotations={value.annotations}
                initialPage={initialPage}
                onPageChange={(page) => onLocationChange({ page })}
                busy={busy}
                onApply={onApply}
              />
            )}
          </WithPagePreviews>
        );
      }}
    </LoadGate>
  );
}
