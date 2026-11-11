"use client";
// What the editor modes need from the workspace: the active document's IR and edit session,
// its pages, form fields and annotations, the page objects Page mode edits, one version store
// for OPFS history, and the redaction rule that purges that history.
import { useCallback, useEffect, useRef, useState } from "react";
import { DocumentEditSession } from "@/editor/document";
import { type IrDocument, irToEditorState } from "@/editor/document/mapping";
import {
  mapEnginePageObjects,
  PageEditSession,
  type PageObjectsModel,
  pageObjectsFromIr,
} from "@/editor/page";
import { createEditorVersionStore, type EditorVersionStore } from "@/editor/storage";
import { type ActiveDocumentData, loadIr, readDocumentData } from "./editor-data";
import type { WorkspaceClient, WorkspaceFile, WorkspaceRun } from "./types";

export type WorkspaceMode = "view" | "annotate" | "document" | "page";

export type Loaded<T> =
  | { status: "loading" }
  | { status: "ready"; value: T }
  | { status: "failed"; message: string };

const messageOf = (reason: unknown) =>
  typeof reason === "object" && reason !== null && "message" in reason
    ? String((reason as { message: unknown }).message)
    : String(reason);

interface Target {
  key: string;
  id: string;
}

/**
 * Loads one value per document key, once. The value stays for the life of the workspace, so an
 * edit session survives a switch to another file and back. `retry` loads a failed value again.
 */
function useKeyedLoader<T>(target: Target | null, load: (target: Target) => Promise<T>) {
  const [entries, setEntries] = useState<Record<string, Loaded<T>>>({});
  const requested = useRef(new Set<string>());
  const loadRef = useRef(load);
  loadRef.current = load;
  const start = useCallback((next: Target) => {
    requested.current.add(next.key);
    setEntries((previous) => ({ ...previous, [next.key]: { status: "loading" } }));
    loadRef.current(next).then(
      (value) =>
        setEntries((previous) => ({ ...previous, [next.key]: { status: "ready", value } })),
      (reason) =>
        setEntries((previous) => ({
          ...previous,
          [next.key]: { status: "failed", message: messageOf(reason) },
        })),
    );
  }, []);
  const key = target?.key;
  const id = target?.id;
  useEffect(() => {
    if (key !== undefined && id !== undefined && !requested.current.has(key)) start({ key, id });
  }, [key, id, start]);
  const retry = useCallback(() => {
    if (key !== undefined && id !== undefined) start({ key, id });
  }, [key, id, start]);
  return { entry: key === undefined ? undefined : entries[key], retry };
}

export interface IrBundle {
  ir: IrDocument;
  /** Shared by Document mode across mode switches, so edits survive them. */
  session: DocumentEditSession;
}

export interface UseEditorWorkspaceOptions {
  client: WorkspaceClient;
  active: WorkspaceFile | null;
  mode: WorkspaceMode;
  run: WorkspaceRun | null;
  budgetBytes?: number;
}

export function useEditorWorkspace({
  client,
  active,
  mode,
  run,
  budgetBytes,
}: UseEditorWorkspaceOptions) {
  const target = (wanted: boolean): Target | null =>
    wanted && active ? { key: active.key, id: active.id } : null;

  // Both Document and Page mode read the IR; one conversion serves both.
  const irPromises = useRef(new Map<string, Promise<IrBundle>>());
  const readIr = useCallback(
    ({ key, id }: Target) => {
      let promise = irPromises.current.get(key);
      if (!promise) {
        promise = loadIr(client, id).then((ir) => ({
          ir,
          session: new DocumentEditSession(irToEditorState(ir)),
        }));
        irPromises.current.set(key, promise);
        promise.catch(() => irPromises.current.delete(key));
      }
      return promise;
    },
    [client],
  );

  const ir = useKeyedLoader<IrBundle>(target(mode === "document" || mode === "page"), readIr);
  const data = useKeyedLoader<ActiveDocumentData>(
    target(mode === "annotate" || mode === "page"),
    ({ id }) => readDocumentData(client, id, budgetBytes),
  );
  // Page objects come from the engine's report when the runtime has it; otherwise from the IR.
  const objects = useKeyedLoader<PageObjectsModel>(target(mode === "page"), async (next) => {
    try {
      const result = await client.runTool(
        "edit.page_objects",
        {},
        { inputIds: [next.id], ...(budgetBytes === undefined ? {} : { budgetBytes }) },
      );
      return mapEnginePageObjects(result.data);
    } catch {
      try {
        return pageObjectsFromIr((await readIr(next)).ir);
      } catch {
        return { pages: [], objects: [], source: "ir" };
      }
    }
  });

  // Page mode's unapplied edits belong to the file, not to the pane, so they survive a mode switch.
  const pageSessions = useRef(new Map<string, PageEditSession>());
  const pageSession = useCallback((key: string, model: PageObjectsModel) => {
    let session = pageSessions.current.get(key);
    if (!session) {
      session = new PageEditSession(model.objects);
      pageSessions.current.set(key, session);
    }
    return session;
  }, []);

  // One OPFS version store for every document, created when Document mode first needs it.
  const [store, setStore] = useState<EditorVersionStore | null>(null);
  const storeRef = useRef<EditorVersionStore | null>(null);
  const ensureStore = useCallback(() => {
    if (!storeRef.current) {
      storeRef.current = createEditorVersionStore();
      setStore(storeRef.current);
    }
    return storeRef.current;
  }, []);
  useEffect(() => {
    if (mode === "document") ensureStore();
  }, [mode, ensureStore]);
  useEffect(
    () => () => {
      const owned = storeRef.current;
      storeRef.current = null;
      void owned?.dispose();
    },
    [],
  );

  // A redaction run makes the earlier states of that document unsafe to keep: they still hold
  // the text that was redacted. The history goes away at once, with or without the editor open.
  const [redacted, setRedacted] = useState<ReadonlySet<string>>(() => new Set());
  // A failed purge belongs to the document it was for, so another document never shows it.
  const [purgeFailure, setPurgeFailure] = useState<{ key: string; message: string } | null>(null);
  // Keyed by the run itself, so a second redaction of the same file purges again.
  const redactedRun = run?.redacts || run?.tool.id === "pdf.redact" ? run : null;
  useEffect(() => {
    const redactedKey = redactedRun?.sourceKey;
    if (!redactedKey) return;
    setRedacted((previous) => new Set(previous).add(redactedKey));
    setPurgeFailure(null);
    ensureStore()
      .purgeDocumentHistory(redactedKey, "redaction")
      .catch((reason) => setPurgeFailure({ key: redactedKey, message: messageOf(reason) }));
  }, [redactedRun, ensureStore]);
  const purgeError = purgeFailure && purgeFailure.key === active?.key ? purgeFailure.message : null;

  return {
    ir,
    data,
    objects,
    pageSession,
    store,
    redacted,
    purgeError,
  };
}
