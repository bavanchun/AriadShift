// Where the worker keeps the input and output files: the origin private file system
// (OPFS) through synchronous access handles, or worker memory when OPFS is missing.
//
// Every worker owns one folder, `ariadshift/<session>/`, and holds a Web Lock named after
// it for as long as it lives. Tabs therefore never share files, and a folder whose lock
// nobody holds belongs to a closed tab or a replaced worker and may be deleted.
import type { ConverterError, OutputLocation, StorageKind } from "./protocol";

/** Top-level OPFS directory owned by the converter. */
export const STORE_DIRECTORY = "ariadshift";
/** Largest input accepted without OPFS. */
export const MEMORY_LIMIT_BYTES = 64 * 1024 * 1024;
/** How long cleanup waits for a replaced worker to release its folder. */
const RETIRE_TIMEOUT_MS = 10_000;

export interface FileStore {
  readonly kind: StorageKind;
  /** Stores the input, replacing any previous one, and returns its size. */
  putInput(file: Blob): Promise<number>;
  /** Reads the input, or its first `maxBytes` bytes. */
  readInput(maxBytes?: number): Promise<Uint8Array>;
  /** Stores the output, replacing any previous one. */
  putOutput(name: string, bytes: Uint8Array, mediaType: string): Promise<OutputLocation>;
  /** Deletes every file of this store. */
  clear(): Promise<void>;
  /**
   * Deletes the folders of replaced workers once they let go of them. Returns how many
   * folders are still held and will be removed by a later sweep.
   */
  retire(sessions: string[]): Promise<number>;
}

function storeError(code: string, message: string, details: object = {}): ConverterError {
  return { code, message, ...details };
}

const noInput = () => storeError("no_input", "Choose a file first.");

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
      throw noInput();
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

  async retire(): Promise<number> {
    return 0;
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

type IterableDirectory = FileSystemDirectoryHandle & { keys(): AsyncIterable<string> };

const INPUT_NAME = "input";

export function sessionLockName(session: string): string {
  return `${STORE_DIRECTORY}:${session}`;
}

async function removeEntry(directory: FileSystemDirectoryHandle, name: string): Promise<void> {
  try {
    await directory.removeEntry(name, { recursive: true });
  } catch (error) {
    if ((error as DOMException).name !== "NotFoundError") {
      throw error;
    }
  }
}

/** Holds the session's lock until the worker ends; resolves once it is held. */
function holdLock(locks: LockManager, session: string): Promise<void> {
  return new Promise((held) => {
    void locks.request(sessionLockName(session), () => {
      held();
      return new Promise<never>(() => {});
    });
  });
}

export class OpfsStore implements FileStore {
  readonly kind = "opfs";

  private constructor(
    private readonly root: FileSystemDirectoryHandle,
    private readonly session: string,
    private readonly locks: LockManager,
  ) {}

  /**
   * Opens this worker's OPFS folder and deletes folders that no live worker holds, or
   * returns null when OPFS with synchronous access handles or Web Locks is unavailable.
   */
  static async open(session: string): Promise<OpfsStore | null> {
    if (
      typeof navigator === "undefined" ||
      typeof navigator.storage?.getDirectory !== "function" ||
      navigator.locks === undefined ||
      typeof FileSystemFileHandle === "undefined" ||
      !("createSyncAccessHandle" in FileSystemFileHandle.prototype)
    ) {
      return null;
    }
    try {
      await holdLock(navigator.locks, session);
      const store = new OpfsStore(await navigator.storage.getDirectory(), session, navigator.locks);
      const directory = await store.directory("input");
      const probe = (await directory.getFileHandle(".probe", { create: true })) as SyncFileHandle;
      (await probe.createSyncAccessHandle()).close();
      await directory.removeEntry(".probe");
      await store.sweep();
      return store;
    } catch {
      return null;
    }
  }

  private async base(): Promise<FileSystemDirectoryHandle> {
    return this.root.getDirectoryHandle(STORE_DIRECTORY, { create: true });
  }

  private async directory(name: "input" | "output"): Promise<FileSystemDirectoryHandle> {
    const own = await (await this.base()).getDirectoryHandle(this.session, { create: true });
    return own.getDirectoryHandle(name, { create: true });
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

  /** Deletes folders whose owners are gone: closed tabs and replaced workers. */
  private async sweep(): Promise<void> {
    const base = await this.base();
    for await (const name of (base as IterableDirectory).keys()) {
      if (name === this.session) {
        continue;
      }
      await this.locks.request(sessionLockName(name), { ifAvailable: true }, async (lock) => {
        if (lock) {
          await removeEntry(base, name);
        }
      });
    }
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
      throw noInput();
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
    for await (const entry of (directory as IterableDirectory).keys()) {
      await removeEntry(directory, entry);
    }
    const access = await this.access("output", name, true);
    try {
      access.truncate(0);
      access.write(bytes, { at: 0 });
      access.flush();
    } finally {
      access.close();
    }
    return { kind: "opfs", directory: [STORE_DIRECTORY, this.session, "output"], name };
  }

  async clear(): Promise<void> {
    await removeEntry(await this.base(), this.session);
  }

  async retire(sessions: string[]): Promise<number> {
    const base = await this.base();
    let held = 0;
    for (const session of sessions) {
      if (session === this.session) {
        continue;
      }
      try {
        await this.locks.request(
          sessionLockName(session),
          { signal: AbortSignal.timeout(RETIRE_TIMEOUT_MS) },
          () => removeEntry(base, session),
        );
      } catch {
        held += 1;
      }
    }
    return held;
  }
}
