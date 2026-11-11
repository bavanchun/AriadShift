"use client";

import {
  type KeyboardEvent,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { createEditorVersionStore, type EditorVersionStore } from "@/editor/storage";
import { AtomEditor } from "./atom-editor";
import { BlockList } from "./block-views";
import { type DocumentModeLabels, resolveLabels } from "./document-mode-labels";
import { DocumentEditSession } from "./document-session";
import { DocumentToolbar } from "./document-toolbar";
import {
  type ActiveAtom,
  EditorContext,
  type EditorContextValue,
  type FocusRequest,
} from "./editor-context";
import {
  ExportFidelityDialog,
  type ExportFormat,
  type ExportRequest,
} from "./export-fidelity-dialog";
import { writeDomSelection } from "./inline-dom";
import {
  type BlockPosition,
  type EditorDocumentState,
  type IrDocument,
  irToEditorState,
} from "./mapping";
import { StylesPanel } from "./styles-panel";
import { useVersionHistory } from "./use-version-history";
import { VersionHistoryDrawer } from "./version-history-drawer";

export interface DocumentModeEditorProps {
  /** Identifies the document in OPFS version history. */
  docId: string;
  /** An `ariad-ir/2` document, as JSON text or an object. */
  ir?: string | IrDocument;
  /** An already mapped state, for example one restored from storage. */
  initialState?: EditorDocumentState;
  /** Supply a session to share edit state with the other editor modes. */
  session?: DocumentEditSession;
  /** Defaults to a store created and disposed by the editor. */
  store?: EditorVersionStore;
  /** Debounce before an edit is autosaved; zero or less turns autosave off. Default 3000. */
  autosaveIntervalMs?: number;
  /**
   * Set when `pdf.redact` ran on this document. Pending autosaves are cancelled at once, all
   * saved versions are purged, and autosave stays off until the document id changes.
   */
  redacted?: boolean;
  onRedactionPurge?: (result: { purgedCount: number }) => void;
  /** Receives the edited IR after the user confirms the fidelity review. */
  onExport?: (request: ExportRequest) => void | Promise<void>;
  defaultExportFormat?: ExportFormat;
  /** Page coordinate (one-based page, top-left points) to select when the editor opens. */
  initialPoint?: { pageNo: number; x: number; y: number };
  /** Reports the selected block and its page position, so another mode can open at the same place. */
  onSelectionLocationChange?: (selection: {
    blockId: string | null;
    location: BlockPosition | null;
  }) => void;
  labels?: Partial<DocumentModeLabels>;
  locale?: string;
  className?: string;
}

/** Word-like reflow editor over the IR's Tiptap JSON model. */
export function DocumentModeEditor({
  docId,
  ir,
  initialState,
  session: sessionProp,
  store: storeProp,
  autosaveIntervalMs,
  redacted,
  onRedactionPurge,
  onExport,
  defaultExportFormat,
  initialPoint,
  onSelectionLocationChange,
  labels: labelOverrides,
  locale,
  className,
}: DocumentModeEditorProps) {
  const labels = useMemo(() => resolveLabels(labelOverrides), [labelOverrides]);
  const [ownedSession] = useState(() => {
    if (sessionProp) return null;
    if (initialState) return new DocumentEditSession(initialState);
    if (ir === undefined) throw new Error("DocumentModeEditor needs ir, initialState or session");
    return new DocumentEditSession(irToEditorState(ir));
  });
  const session = sessionProp ?? (ownedSession as DocumentEditSession);
  const snapshot = useSyncExternalStore(
    session.subscribe,
    session.getSnapshot,
    session.getSnapshot,
  );

  const [ownedStore, setOwnedStore] = useState<EditorVersionStore | null>(null);
  useEffect(() => {
    if (storeProp) return;
    const created = createEditorVersionStore();
    setOwnedStore(created);
    return () => {
      setOwnedStore(null);
      void created.dispose();
    };
  }, [storeProp]);
  const store = storeProp ?? ownedStore;

  const history = useVersionHistory({
    docId,
    store,
    session,
    autosaveIntervalMs,
    redacted,
    onRedactionPurge,
  });

  const [stylesOpen, setStylesOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  const [linkOpen, setLinkOpen] = useState(false);
  const [announcement, setAnnouncement] = useState("");
  const [activeAtom, setActiveAtom] = useState<ActiveAtom | null>(null);
  const focusRequest = useRef<FocusRequest | null>(null);
  const canvas = useRef<HTMLDivElement>(null);
  const scrollTarget = useRef<string | null>(null);

  const styles = snapshot.state.original.styles;
  const assets = snapshot.state.original.assets;
  const context = useMemo<EditorContextValue>(
    () => ({
      session,
      styles,
      assets,
      labels,
      focusRequest,
      activateAtom: setActiveAtom,
      announce: setAnnouncement,
    }),
    [session, styles, assets, labels],
  );

  // Open at the block nearest the page position the previous mode was showing.
  useLayoutEffect(() => {
    if (!initialPoint) return;
    const id = session.selectNearestPoint(initialPoint.pageNo, initialPoint.x, initialPoint.y);
    scrollTarget.current = id;
  }, []);

  const selectedBlockId = snapshot.selectedBlockId;
  const onLocationChange = useRef(onSelectionLocationChange);
  onLocationChange.current = onSelectionLocationChange;
  useEffect(() => {
    onLocationChange.current?.({
      blockId: selectedBlockId,
      location: session.getSelectionLocation(),
    });
  }, [selectedBlockId, session]);

  // Moves focus to the block an operation selected (split, merge, undo) once it has rendered.
  useLayoutEffect(() => {
    const root = canvas.current;
    if (!root) return;
    if (scrollTarget.current) {
      const element = root.querySelector(`[data-block-id="${scrollTarget.current}"]`);
      scrollTarget.current = null;
      element?.scrollIntoView?.({ block: "center" });
    }
    const request = focusRequest.current;
    if (!request) return;
    focusRequest.current = null;
    const selection = session.getSelection();
    let blockId: string | null;
    let part: string;
    if (request === "selection") {
      blockId = selection?.blockId ?? session.getSelectedBlockId();
      part = selection?.part ?? "content";
    } else {
      blockId = request.blockId;
      part = request.part;
    }
    if (!blockId) return;
    const element = root.querySelector<HTMLElement>(
      `[data-inline-editable][data-block-id="${blockId}"][data-part="${part}"]`,
    );
    if (!element) return;
    element.focus();
    if (selection && selection.blockId === blockId && selection.part === part) {
      writeDomSelection(element, selection.from, selection.to);
      // Focusing reports a caret at the start; the intended range is the truth.
      session.setSelection(selection);
    }
  }, [snapshot]);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const key = event.key.toLowerCase();
    const mod = event.metaKey || event.ctrlKey;
    if (mod && key === "z") {
      event.preventDefault();
      if (event.shiftKey) session.redo();
      else session.undo();
    } else if (mod && key === "y") {
      event.preventDefault();
      session.redo();
    } else if (mod && !event.shiftKey && ["b", "i", "u"].includes(key)) {
      if ((event.target as HTMLElement).closest("[data-inline-editable]") === null) return;
      event.preventDefault();
      session.toggleMark(key === "b" ? "bold" : key === "i" ? "italic" : "underline");
    } else if (mod && key === "k") {
      event.preventDefault();
      setLinkOpen((open) => !open);
    } else if (event.altKey && (key === "arrowup" || key === "arrowdown")) {
      if (!session.getSelectedBlockId()) return;
      event.preventDefault();
      const up = key === "arrowup";
      focusRequest.current = "selection";
      if (session.moveBlock(up ? -1 : 1)) {
        setAnnouncement(up ? labels.announceMovedUp : labels.announceMovedDown);
      } else focusRequest.current = null;
    }
  };

  const saveCheckpoint = async () => {
    if (await history.saveCheckpoint()) setAnnouncement(labels.announceCheckpoint);
  };

  return (
    <EditorContext.Provider value={context}>
      <section
        aria-label={labels.editorLabel}
        data-testid="document-mode-editor"
        className={`flex min-h-0 flex-col bg-[var(--color-bg-canvas)] text-foreground ${className ?? ""}`}
        onKeyDown={onKeyDown}
      >
        <DocumentToolbar
          session={session}
          snapshot={snapshot}
          labels={labels}
          stylesOpen={stylesOpen}
          historyOpen={historyOpen}
          linkOpen={linkOpen}
          onToggleLink={() => setLinkOpen((open) => !open)}
          onToggleStyles={() => setStylesOpen((open) => !open)}
          onToggleHistory={() => setHistoryOpen((open) => !open)}
          onSaveCheckpoint={() => void saveCheckpoint()}
          onExport={() => setExportOpen(true)}
          autosave={{ status: history.status, lastSavedAt: history.lastSavedAt }}
          locale={locale}
          announce={setAnnouncement}
        />
        {activeAtom && (
          <AtomEditor
            atom={activeAtom}
            snapshot={snapshot}
            session={session}
            labels={labels}
            onClose={() => setActiveAtom(null)}
          />
        )}
        <div className="flex min-h-0 flex-1">
          <div className="min-w-0 flex-1 overflow-y-auto p-4">
            <section
              ref={canvas}
              aria-label={labels.canvasLabel}
              data-document-canvas=""
              className="mx-auto max-w-3xl rounded-lg border border-[var(--color-border-subtle)] bg-[var(--color-bg-surface)] p-8 shadow-sm"
            >
              {snapshot.state.doc.content.length === 0 ? (
                <p className="text-sm text-muted-foreground">{labels.emptyDocument}</p>
              ) : (
                <BlockList nodes={snapshot.state.doc.content} />
              )}
            </section>
          </div>
          {stylesOpen && <StylesPanel session={session} snapshot={snapshot} labels={labels} />}
          {historyOpen && (
            <VersionHistoryDrawer history={history} labels={labels} locale={locale} />
          )}
        </div>
        <div role="status" aria-live="polite" className="sr-only">
          {announcement}
        </div>
        <ExportFidelityDialog
          open={exportOpen}
          onOpenChange={setExportOpen}
          session={session}
          revision={snapshot.revision}
          labels={labels}
          onExport={async (request) => {
            await onExport?.(request);
          }}
          defaultFormat={defaultExportFormat}
        />
      </section>
    </EditorContext.Provider>
  );
}
