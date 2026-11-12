import type { EditorVersionBackend, EditorVersionEntry, StoredEditorVersion } from "./types";

interface SyncHandle {
  read(buffer: Uint8Array, options?: { at: number }): number;
  write(buffer: Uint8Array, options?: { at: number }): number;
  getSize(): number;
  truncate(size: number): void;
  flush(): void;
  close(): void;
}

type SyncFile = FileSystemFileHandle & { createSyncAccessHandle(): Promise<SyncHandle> };
type Directory = FileSystemDirectoryHandle & { keys(): AsyncIterableIterator<string> };

function isMissing(error: unknown): boolean {
  return error instanceof Error && error.name === "NotFoundError";
}

/** Used inside version-worker; file bytes never use main-thread createWritable. */
export class OpfsVersionBackend implements EditorVersionBackend {
  constructor(private readonly root: FileSystemDirectoryHandle) {}

  private async directory(
    docId: string,
    create = false,
  ): Promise<FileSystemDirectoryHandle | null> {
    // Hash arbitrary user identifiers into bounded, collision-resistant path segments.
    const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(docId));
    const name = Array.from(new Uint8Array(digest), (byte) =>
      byte.toString(16).padStart(2, "0"),
    ).join("");
    try {
      const base = await this.root.getDirectoryHandle("ariadshift-editor-versions", { create });
      return await base.getDirectoryHandle(name, { create });
    } catch (error) {
      if (!create && isMissing(error)) return null;
      throw error;
    }
  }

  private filename(versionId: string): string {
    if (!/^[A-Za-z0-9-]+$/.test(versionId)) throw new Error("Invalid editor version ID");
    return `${versionId}.json`;
  }

  async list(docId: string): Promise<EditorVersionEntry[]> {
    const directory = await this.directory(docId);
    if (!directory) return [];
    const entries: EditorVersionEntry[] = [];
    for await (const name of (directory as Directory).keys()) {
      if (!name.endsWith(".json")) continue;
      const version = await this.read(docId, name.slice(0, -5));
      if (version) entries.push(version.entry);
    }
    return entries;
  }

  async read(docId: string, versionId: string): Promise<StoredEditorVersion | null> {
    const directory = await this.directory(docId);
    if (!directory) return null;
    let file: SyncFile;
    try {
      file = (await directory.getFileHandle(this.filename(versionId))) as SyncFile;
    } catch (error) {
      if (isMissing(error)) return null;
      throw error;
    }
    const handle = await file.createSyncAccessHandle();
    try {
      const bytes = new Uint8Array(handle.getSize());
      let offset = 0;
      while (offset < bytes.byteLength) {
        const count = handle.read(bytes.subarray(offset), { at: offset });
        if (!count) throw new Error("Incomplete editor version read");
        offset += count;
      }
      return JSON.parse(new TextDecoder().decode(bytes)) as StoredEditorVersion;
    } finally {
      handle.close();
    }
  }

  async write(docId: string, version: StoredEditorVersion): Promise<void> {
    const directory = await this.directory(docId, true);
    if (!directory) throw new Error("Editor version directory unavailable");
    const name = this.filename(version.entry.versionId);
    const file = (await directory.getFileHandle(name, { create: true })) as SyncFile;
    try {
      const handle = await file.createSyncAccessHandle();
      try {
        const bytes = new TextEncoder().encode(JSON.stringify(version));
        let offset = 0;
        while (offset < bytes.byteLength) {
          const count = handle.write(bytes.subarray(offset), { at: offset });
          if (!count) throw new Error("Incomplete editor version write");
          offset += count;
        }
        handle.truncate(bytes.byteLength);
        handle.flush();
      } finally {
        handle.close();
      }
    } catch (error) {
      await directory.removeEntry(name);
      throw error;
    }
  }

  async remove(docId: string, versionId: string): Promise<void> {
    const directory = await this.directory(docId);
    if (!directory) return;
    try {
      await directory.removeEntry(this.filename(versionId));
    } catch (error) {
      if (!isMissing(error)) throw error;
    }
  }

  async purge(docId: string): Promise<number> {
    const directory = await this.directory(docId);
    if (!directory) return 0;
    let count = 0;
    // Count physical snapshots, even a corrupt/partly-written envelope. Never parse
    // history before purging it: corrupt JSON must not prevent secure deletion.
    for await (const name of (directory as Directory).keys()) if (name.endsWith(".json")) count++;
    const base = await this.root.getDirectoryHandle("ariadshift-editor-versions");
    await base.removeEntry(directory.name, { recursive: true });
    return count;
  }
}
