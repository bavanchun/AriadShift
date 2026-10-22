// Where the worker keeps the input and output files: the origin private file system
// (OPFS) through synchronous access handles, or worker memory when OPFS is missing.
import type { ConverterError, OutputLocation, StorageKind } from "./protocol";

/** Top-level OPFS directory owned by the converter. */
export const STORE_DIRECTORY = "ariadshift";
/** Largest input accepted without OPFS. */
export const MEMORY_LIMIT_BYTES = 64 * 1024 * 1024;

export interface FileStore {
  readonly kind: StorageKind;
  /** Stores the input, replacing any previous one, and returns its size. */
  putInput(file: Blob): Promise<number>;
  /** Reads the input, or its first `maxBytes` bytes. */
  readInput(maxBytes?: number): Promise<Uint8Array>;
  /** Stores the output, replacing any previous one. */
  putOutput(name: string, bytes: Uint8Array, mediaType: string): Promise<OutputLocation>;
  /** Deletes every stored file. */
  clear(): Promise<void>;
}

function storeError(code: string, message: string, details: object = {}): ConverterError {
  return { code, message, ...details };
}

export class MemoryStore implements FileStore {
  readonly kind = "memory";
  #input: Blob | null = null;

  constructor(readonly limitBytes = MEMORY_LIMIT_BYTES) {}

  async putInput(file: Blob): Promise<number> {
    if (file.size > this.limitBytes) {
      throw storeError(
        "storage_limit",
        `This browser cannot store files on the device, so inputs above ${Math.round(this.limitBytes / 1024 / 1024)} MB are not supported here.`,
        { limit: this.limitBytes },
      );
    }
    this.#input = file;
    return file.size;
  }

  async readInput(maxBytes?: number): Promise<Uint8Array> {
    if (!this.#input) {
      throw storeError("no_input", "Choose a file first.");
    }
    const blob = maxBytes === undefined ? this.#input : this.#input.slice(0, maxBytes);
    return new Uint8Array(await blob.arrayBuffer());
  }

  async putOutput(_name: string, bytes: Uint8Array, mediaType: string): Promise<OutputLocation> {
    return {
      kind: "memory",
      blob: new Blob([bytes as Uint8Array<ArrayBuffer>], { type: mediaType }),
    };
  }

  async clear(): Promise<void> {
    this.#input = null;
  }
}

interface SyncAccessHandle {
  read(buffer: Uint8Array, options?: { at?: number }): number;
  write(buffer: Uint8Array, options?: { at?: number }): number;
  truncate(size: number): void;
  getSize(): number;
  flush(): void;
  close(): void;
}

type SyncFileHandle = FileSystemFileHandle & {
  createSyncAccessHandle(): Promise<SyncAccessHandle>;
};

const INPUT_NAME = "input";

export class OpfsStore implements FileStore {
  readonly kind = "opfs";

  private constructor(private readonly root: FileSystemDirectoryHandle) {}

  /** Opens OPFS with synchronous access handles, or returns null when unavailable. */
  static async open(): Promise<OpfsStore | null> {
    if (
      typeof navigator === "undefined" ||
      typeof navigator.storage?.getDirectory !== "function" ||
      typeof FileSystemFileHandle === "undefined" ||
      !("createSyncAccessHandle" in FileSystemFileHandle.prototype)
    ) {
      return null;
    }
    try {
      const store = new OpfsStore(await navigator.storage.getDirectory());
      const directory = await store.directory("input");
      const probe = (await directory.getFileHandle(".probe", { create: true })) as SyncFileHandle;
      (await probe.createSyncAccessHandle()).close();
      await directory.removeEntry(".probe");
      return store;
    } catch {
      return null;
    }
  }

  private async directory(name: "input" | "output"): Promise<FileSystemDirectoryHandle> {
    const base = await this.root.getDirectoryHandle(STORE_DIRECTORY, { create: true });
    return base.getDirectoryHandle(name, { create: true });
  }

  private async access(
    name: "input" | "output",
    file: string,
    create: boolean,
  ): Promise<SyncAccessHandle> {
    const directory = await this.directory(name);
    const handle = (await directory.getFileHandle(file, { create })) as SyncFileHandle;
    return handle.createSyncAccessHandle();
  }

  async putInput(file: Blob): Promise<number> {
    const access = await this.access("input", INPUT_NAME, true);
    try {
      access.truncate(0);
      let offset = 0;
      const reader = file.stream().getReader();
      for (;;) {
        const { done, value } = await reader.read();
        if (done) {
          break;
        }
        offset += access.write(value, { at: offset });
      }
      access.flush();
      return offset;
    } finally {
      access.close();
    }
  }

  async readInput(maxBytes?: number): Promise<Uint8Array> {
    let access: SyncAccessHandle;
    try {
      access = await this.access("input", INPUT_NAME, false);
    } catch {
      throw storeError("no_input", "Choose a file first.");
    }
    try {
      const size = access.getSize();
      const buffer = new Uint8Array(maxBytes === undefined ? size : Math.min(size, maxBytes));
      access.read(buffer, { at: 0 });
      return buffer;
    } finally {
      access.close();
    }
  }

  async putOutput(name: string, bytes: Uint8Array): Promise<OutputLocation> {
    const directory = await this.directory("output");
    for await (const entry of (
      directory as FileSystemDirectoryHandle & { keys(): AsyncIterable<string> }
    ).keys()) {
      await directory.removeEntry(entry);
    }
    const access = await this.access("output", name, true);
    try {
      access.write(bytes, { at: 0 });
      access.flush();
    } finally {
      access.close();
    }
    return { kind: "opfs", directory: [STORE_DIRECTORY, "output"], name };
  }

  async clear(): Promise<void> {
    try {
      await this.root.removeEntry(STORE_DIRECTORY, { recursive: true });
    } catch (error) {
      if ((error as DOMException).name !== "NotFoundError") {
        throw error;
      }
    }
  }
}
