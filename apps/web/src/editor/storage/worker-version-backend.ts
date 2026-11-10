import type {
  EditorVersionBackend,
  EditorVersionEntry,
  StoredEditorVersion,
  VersionWorkerReply,
  VersionWorkerRequest,
} from "./types";

export class WorkerVersionBackend implements EditorVersionBackend {
  private sequence = 0;
  private failure: Error | null = null;
  private readonly pending = new Map<
    number,
    {
      resolve(value: unknown): void;
      reject(error: Error): void;
    }
  >();

  private constructor(private readonly worker: Worker) {
    worker.onmessage = (event: MessageEvent<VersionWorkerReply>) => {
      const { requestId, result, error } = event.data;
      const pending = this.pending.get(requestId);
      this.pending.delete(requestId);
      if (error !== undefined) pending?.reject(new Error(error));
      else pending?.resolve(result);
    };
    worker.onerror = () => this.fail(new Error("Editor version worker failed"));
    worker.onmessageerror = () => this.fail(new Error("Editor version worker message failed"));
  }

  static async open(): Promise<WorkerVersionBackend | null> {
    if (
      typeof Worker === "undefined" ||
      typeof navigator === "undefined" ||
      typeof navigator.storage?.getDirectory !== "function"
    )
      return null;
    const backend = new WorkerVersionBackend(
      new Worker(new URL("./version-worker.ts", import.meta.url), {
        type: "module",
        name: "ariadshift-editor-versions",
      }),
    );
    try {
      if (await backend.request({ method: "open", docId: "" })) return backend;
    } catch (error) {
      backend.dispose();
      // Once OPFS is advertised, access failures must remain visible. Falling
      // back here could let a redaction purge only memory while old OPFS
      // snapshots remain inaccessible.
      throw error;
    }
    // The memory fallback still purges reachable OPFS directories so snapshots
    // from an earlier browser with sync support cannot be concealed.
    backend.dispose();
    return null;
  }

  private request(request: Omit<VersionWorkerRequest, "requestId">): Promise<unknown> {
    if (this.failure) return Promise.reject(this.failure);
    const requestId = ++this.sequence;
    return new Promise((resolve, reject) => {
      this.pending.set(requestId, { resolve, reject });
      try {
        this.worker.postMessage({ ...request, requestId });
      } catch (error) {
        this.pending.delete(requestId);
        reject(error);
      }
    });
  }

  private fail(error: Error): void {
    this.failure = error;
    for (const pending of this.pending.values()) pending.reject(error);
    this.pending.clear();
    this.worker.terminate();
  }

  dispose(): void {
    this.fail(new Error("Editor version store is closed"));
  }

  async list(docId: string): Promise<EditorVersionEntry[]> {
    return (await this.request({ method: "list", docId })) as EditorVersionEntry[];
  }

  async read(docId: string, versionId: string): Promise<StoredEditorVersion | null> {
    return (await this.request({ method: "read", docId, versionId })) as StoredEditorVersion | null;
  }

  async write(docId: string, version: StoredEditorVersion): Promise<void> {
    await this.request({ method: "write", docId, version });
  }

  async remove(docId: string, versionId: string): Promise<void> {
    await this.request({ method: "remove", docId, versionId });
  }

  async purge(docId: string): Promise<number> {
    return (await this.request({ method: "purge", docId })) as number;
  }
}
