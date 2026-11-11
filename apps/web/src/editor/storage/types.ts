import type { EditorDocumentState } from "../document/mapping/types";

export interface EditorVersionEntry {
  versionId: string;
  docId: string;
  createdAt: string;
  kind: "autosave" | "manual";
  label?: string;
  sizeBytes: number;
}

/** A single envelope avoids a separate index that could hide orphan snapshots. */
export interface StoredEditorVersion {
  entry: EditorVersionEntry;
  state: EditorDocumentState;
}

export interface EditorVersionBackend {
  list(docId: string): Promise<EditorVersionEntry[]>;
  read(docId: string, versionId: string): Promise<StoredEditorVersion | null>;
  write(docId: string, version: StoredEditorVersion): Promise<void>;
  remove(docId: string, versionId: string): Promise<void>;
  purge(docId: string): Promise<number>;
  dispose?(): void;
}

export interface EditorVersionStoreOptions {
  maxVersionsPerDoc?: number;
  backend?: EditorVersionBackend;
}

export type VersionWorkerRequest = {
  requestId: number;
  method: "open" | "list" | "read" | "write" | "remove" | "purge";
  docId: string;
  versionId?: string;
  version?: StoredEditorVersion;
};

export type VersionWorkerReply = { requestId: number; result?: unknown; error?: string };
