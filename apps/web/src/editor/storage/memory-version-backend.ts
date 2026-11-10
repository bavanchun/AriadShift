import type { EditorVersionBackend, EditorVersionEntry, StoredEditorVersion } from "./types";

export class MemoryVersionBackend implements EditorVersionBackend {
  private readonly documents = new Map<string, Map<string, string>>();

  /** Useful for verifying that redaction removed payloads, not just index entries. */
  get sizeBytes(): number {
    let bytes = 0;
    for (const versions of this.documents.values()) {
      for (const value of versions.values()) bytes += new TextEncoder().encode(value).byteLength;
    }
    return bytes;
  }

  async list(docId: string): Promise<EditorVersionEntry[]> {
    return [...(this.documents.get(docId)?.values() ?? [])].map(
      (value) => (JSON.parse(value) as StoredEditorVersion).entry,
    );
  }

  async read(docId: string, versionId: string): Promise<StoredEditorVersion | null> {
    const value = this.documents.get(docId)?.get(versionId);
    return value ? (JSON.parse(value) as StoredEditorVersion) : null;
  }

  async write(docId: string, version: StoredEditorVersion): Promise<void> {
    let versions = this.documents.get(docId);
    if (!versions) {
      versions = new Map();
      this.documents.set(docId, versions);
    }
    versions.set(version.entry.versionId, JSON.stringify(version));
  }

  async remove(docId: string, versionId: string): Promise<void> {
    const versions = this.documents.get(docId);
    versions?.delete(versionId);
    if (!versions?.size) this.documents.delete(docId);
  }

  async purge(docId: string): Promise<number> {
    const count = this.documents.get(docId)?.size ?? 0;
    this.documents.delete(docId);
    return count;
  }
}
