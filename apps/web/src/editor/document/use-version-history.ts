"use client";

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { EditorVersionEntry, EditorVersionStore } from "@/editor/storage";
import type { DocumentEditSession } from "./document-session";

export type AutosaveStatus = "idle" | "pending" | "saving" | "saved" | "error" | "off" | "purged";

export interface UseVersionHistoryOptions {
  docId: string;
  /** Null while the owner is still creating the store. */
  store: EditorVersionStore | null;
  session: DocumentEditSession;
  /** Debounce before an edit is autosaved. Zero or less turns autosave off. */
  autosaveIntervalMs?: number;
  /** When true, pending autosaves are cancelled at once and the history is purged. */
  redacted?: boolean;
  onRestore?: () => void;
  onRedactionPurge?: (result: { purgedCount: number }) => void;
}

export interface VersionHistory {
  versions: EditorVersionEntry[];
  status: AutosaveStatus;
  lastSavedAt: string | null;
  error: string | null;
  /** Number of versions removed by the last redaction purge, or null before one ran. */
  purgedCount: number | null;
  /** True after a redaction purge: nothing is saved for this document any more. */
  locked: boolean;
  refresh: () => Promise<void>;
  saveCheckpoint: (label?: string) => Promise<EditorVersionEntry | null>;
  restore: (versionId: string) => Promise<boolean>;
  /** Cancels pending autosaves first, then deletes every saved version of the document. */
  purgeForRedaction: () => Promise<number | null>;
  cancelAutosave: () => void;
}

const DEFAULT_INTERVAL_MS = 3000;

const messageOf = (error: unknown) => (error instanceof Error ? error.message : String(error));

export function useVersionHistory({
  docId,
  store,
  session,
  autosaveIntervalMs = DEFAULT_INTERVAL_MS,
  redacted = false,
  onRestore,
  onRedactionPurge,
}: UseVersionHistoryOptions): VersionHistory {
  const enabled = autosaveIntervalMs > 0;
  const [versions, setVersions] = useState<EditorVersionEntry[]>([]);
  const [status, setStatus] = useState<AutosaveStatus>(enabled ? "idle" : "off");
  const [lastSavedAt, setLastSavedAt] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [purgedCount, setPurgedCount] = useState<number | null>(null);
  const [locked, setLocked] = useState(false);
  const lockedRef = useRef(redacted);
  // Stop future saves during render, before any effect or timer can write stale state.
  if (redacted) lockedRef.current = true;
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const savedRevision = useRef(session.getSnapshot().revision);
  const mounted = useRef(true);
  const callbacks = useRef({ onRestore, onRedactionPurge });
  callbacks.current = { onRestore, onRedactionPurge };

  const cancelAutosave = useCallback(() => {
    if (timer.current !== null) clearTimeout(timer.current);
    timer.current = null;
  }, []);

  const refresh = useCallback(async () => {
    if (!store) return;
    try {
      const entries = await store.listVersions(docId);
      if (mounted.current && !lockedRef.current) setVersions(entries);
    } catch (reason) {
      if (mounted.current) {
        setError(messageOf(reason));
        setStatus("error");
      }
    }
  }, [store, docId]);

  // A different document starts with a fresh, unlocked history.
  useEffect(() => {
    mounted.current = true;
    lockedRef.current = redacted;
    setLocked(redacted);
    setPurgedCount(null);
    setError(null);
    setLastSavedAt(null);
    setStatus(enabled ? "idle" : "off");
    savedRevision.current = session.getSnapshot().revision;
    void refresh();
    return () => {
      mounted.current = false;
      cancelAutosave();
    };
  }, [docId, session, store]);

  const autosave = useCallback(async () => {
    timer.current = null;
    if (lockedRef.current || !store) return;
    const revision = session.getSnapshot().revision;
    setStatus("saving");
    try {
      const entry = await store.saveVersion(docId, session.getState(), { kind: "autosave" });
      if (!mounted.current || lockedRef.current) return;
      savedRevision.current = revision;
      setLastSavedAt(entry.createdAt);
      setError(null);
      // Edits made while saving schedule their own save through the subscription.
      setStatus(session.getSnapshot().revision === revision ? "saved" : "pending");
      await refresh();
    } catch (reason) {
      if (!mounted.current) return;
      setError(messageOf(reason));
      setStatus("error");
    }
  }, [docId, refresh, session, store]);

  useEffect(() => {
    if (!enabled) {
      cancelAutosave();
      setStatus((current) => (current === "purged" ? current : "off"));
      return;
    }
    setStatus((current) => (current === "off" ? "idle" : current));
    return session.subscribe(() => {
      if (lockedRef.current) return;
      if (session.getSnapshot().revision === savedRevision.current) return;
      cancelAutosave();
      setStatus("pending");
      timer.current = setTimeout(() => void autosave(), autosaveIntervalMs);
    });
  }, [autosave, autosaveIntervalMs, cancelAutosave, enabled, session]);

  const saveCheckpoint = useCallback(
    async (label?: string) => {
      if (lockedRef.current || !store) return null;
      try {
        const entry = await store.saveVersion(docId, session.getState(), {
          kind: "manual",
          ...(label?.trim() ? { label: label.trim() } : {}),
        });
        if (mounted.current && !lockedRef.current) {
          setLastSavedAt(entry.createdAt);
          setError(null);
        }
        await refresh();
        return entry;
      } catch (reason) {
        if (mounted.current) {
          setError(messageOf(reason));
          setStatus("error");
        }
        return null;
      }
    },
    [docId, refresh, session, store],
  );

  const restore = useCallback(
    async (versionId: string) => {
      if (!store) return false;
      try {
        const state = await store.restoreVersion(docId, versionId);
        session.replaceState(state);
        callbacks.current.onRestore?.();
        return true;
      } catch (reason) {
        if (mounted.current) setError(messageOf(reason));
        return false;
      }
    },
    [docId, session, store],
  );

  const purgeForRedaction = useCallback(async () => {
    // Stop future saves before anything is awaited, so stale state cannot be written back.
    lockedRef.current = true;
    cancelAutosave();
    setLocked(true);
    setStatus("purged");
    if (!store) return null;
    try {
      const { purgedCount: count } = await store.purgeDocumentHistory(docId, "redaction");
      if (mounted.current) {
        setVersions([]);
        setPurgedCount(count);
        setError(null);
      }
      callbacks.current.onRedactionPurge?.({ purgedCount: count });
      return count;
    } catch (reason) {
      if (mounted.current) setError(messageOf(reason));
      return null;
    }
  }, [cancelAutosave, docId, store]);

  useEffect(() => {
    if (redacted) void purgeForRedaction();
  }, [redacted, purgeForRedaction]);

  return useMemo(
    () => ({
      versions,
      status,
      lastSavedAt,
      error,
      purgedCount,
      locked,
      refresh,
      saveCheckpoint,
      restore,
      purgeForRedaction,
      cancelAutosave,
    }),
    [
      versions,
      status,
      lastSavedAt,
      error,
      purgedCount,
      locked,
      refresh,
      saveCheckpoint,
      restore,
      purgeForRedaction,
      cancelAutosave,
    ],
  );
}
