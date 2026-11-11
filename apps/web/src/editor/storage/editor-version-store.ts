import type { EditorDocumentState } from "../document/mapping/types";
import { MemoryVersionBackend } from "./memory-version-backend";
import { OpfsVersionBackend } from "./opfs-version-backend";
import type { EditorVersionBackend, EditorVersionEntry, EditorVersionStoreOptions } from "./types";
import { WorkerVersionBackend } from "./worker-version-backend";

const backendQueues = new WeakMap<EditorVersionBackend, Map<string, Promise<unknown>>>();
let sequence = 0;

/** A browser can lose worker/sync support after having persisted older versions. */
class FallbackMemoryBackend extends MemoryVersionBackend {
  override async purge(docId: string): Promise<number> {
    let persisted = 0;
    if (typeof navigator !== "undefined" && typeof navigator.storage?.getDirectory === "function") {
      // Directory deletion needs no sync handle and reads no document bytes on
      // the main thread. A permission failure must prevent a successful purge.
      persisted = await new OpfsVersionBackend(await navigator.storage.getDirectory()).purge(docId);
    }
    return persisted + (await super.purge(docId));
  }
}

function newestFirst(entries: EditorVersionEntry[]): EditorVersionEntry[] {
  return entries.sort(
    (a, b) => b.createdAt.localeCompare(a.createdAt) || b.versionId.localeCompare(a.versionId),
  );
}

export class EditorVersionStore {
  private backend: Promise<EditorVersionBackend> | undefined;
  private readonly maxVersions: number;
  private closed = false;

  constructor(options: EditorVersionStoreOptions = {}) {
    this.maxVersions = options.maxVersionsPerDoc ?? 25;
    if (!Number.isSafeInteger(this.maxVersions) || this.maxVersions < 1) {
      throw new Error("maxVersionsPerDoc must be a positive integer");
    }
    if (options.backend) this.backend = Promise.resolve(options.backend);
  }

  private getBackend(): Promise<EditorVersionBackend> {
    this.backend ??= WorkerVersionBackend.open().then(
      (backend) => backend ?? new FallbackMemoryBackend(),
    );
    return this.backend;
  }

  private async withDocument<T>(
    docId: string,
    operation: (backend: EditorVersionBackend) => Promise<T>,
  ): Promise<T> {
    if (this.closed) throw new Error("Editor version store is closed");
    if (!docId) throw new Error("Document ID must not be empty");
    const backend = await this.getBackend();
    let queues = backendQueues.get(backend);
    if (!queues) {
      queues = new Map();
      backendQueues.set(backend, queues);
    }
    const previous = queues.get(docId) ?? Promise.resolve();
    const current = previous
      .catch(() => undefined)
      .then(() => {
        // The lock covers the whole save/prune or purge transaction across tabs.
        if (typeof navigator !== "undefined" && navigator.locks) {
          return navigator.locks.request(`ariadshift-editor-history:${docId}`, () =>
            operation(backend),
          );
        }
        return operation(backend);
      });
    queues.set(docId, current);
    try {
      return await current;
    } finally {
      if (queues.get(docId) === current) queues.delete(docId);
    }
  }

  private async prune(
    backend: EditorVersionBackend,
    docId: string,
    keepId?: string,
  ): Promise<EditorVersionEntry[]> {
    const entries = newestFirst(await backend.list(docId));
    const candidates = entries.filter((entry) => entry.versionId !== keepId).reverse();
    // Prefer retaining manual checkpoints. A strict cap also bounds all-manual
    // histories; in that case the oldest manual checkpoint is removed.
    const oldest = [
      ...candidates.filter((entry) => entry.kind === "autosave"),
      ...candidates.filter((entry) => entry.kind === "manual"),
    ];
    const removed = new Set<string>();
    for (const entry of oldest.slice(0, Math.max(0, entries.length - this.maxVersions))) {
      await backend.remove(docId, entry.versionId);
      removed.add(entry.versionId);
    }
    return entries.filter((entry) => !removed.has(entry.versionId));
  }

  async saveVersion(
    docId: string,
    state: EditorDocumentState,
    options: { label?: string; kind?: "autosave" | "manual" } = {},
  ): Promise<EditorVersionEntry> {
    // Capture immediately; queued saves must not observe later editor mutations.
    const savedState = structuredClone(state);
    const savedOptions = { ...options };
    return this.withDocument(docId, async (backend) => {
      const existing = newestFirst(await backend.list(docId));
      const lastTime = Date.parse(existing[0]?.createdAt ?? "") || 0;
      const now = Math.max(Date.now(), lastTime + 1);
      const versionId =
        typeof crypto !== "undefined" && typeof crypto.randomUUID === "function"
          ? crypto.randomUUID()
          : `v${now.toString(36)}-${(++sequence).toString(36)}-${Math.random().toString(36).slice(2)}`;
      const entry: EditorVersionEntry = {
        docId,
        versionId,
        createdAt: new Date(now).toISOString(),
        kind: savedOptions.kind ?? "autosave",
        sizeBytes: new TextEncoder().encode(JSON.stringify(savedState)).byteLength,
        ...(savedOptions.label !== undefined ? { label: savedOptions.label } : {}),
      };
      await backend.write(docId, { entry, state: savedState });
      await this.prune(backend, docId, versionId);
      return structuredClone(entry);
    });
  }

  async listVersions(docId: string): Promise<EditorVersionEntry[]> {
    return this.withDocument(docId, (backend) => this.prune(backend, docId));
  }

  async loadVersion(docId: string, versionId: string): Promise<EditorDocumentState | null> {
    return this.withDocument(docId, async (backend) => {
      const version = await backend.read(docId, versionId);
      return version ? structuredClone(version.state) : null;
    });
  }

  async restoreVersion(docId: string, versionId: string): Promise<EditorDocumentState> {
    const state = await this.loadVersion(docId, versionId);
    if (!state) throw new Error(`Editor version not found: ${versionId}`);
    return state;
  }

  async purgeDocumentHistory(
    docId: string,
    reason: "redaction" | "user",
  ): Promise<{ purgedCount: number; reason: string }> {
    return this.withDocument(docId, async (backend) => ({
      purgedCount: await backend.purge(docId),
      reason,
    }));
  }

  /** Call when closing the editor to release its storage worker. */
  async dispose(): Promise<void> {
    this.closed = true;
    const backend = await this.backend?.catch(() => null);
    if (!backend) return;
    await Promise.allSettled(backendQueues.get(backend)?.values() ?? []);
    backend.dispose?.();
  }
}

export function createEditorVersionStore(options?: EditorVersionStoreOptions): EditorVersionStore {
  return new EditorVersionStore(options);
}
