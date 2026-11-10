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

export interface InputSlot {
  id: string;
  name: string;
  size: number;
}

export interface StoredInput {
  id: string;
  name: string;
  bytes: Uint8Array;
}

export interface StoredArtifact {
  name: string;
  bytes: Uint8Array;
  mediaType: string;
}

/** Aggregate input limit, shared by the store and tool runner. */
export const STORE_INPUT_LIMIT_BYTES = 16 * 1024 * 1024;

function totalBytes(items: Array<{ bytes: Uint8Array }>): number {
  return items.reduce((sum, item) => sum + item.bytes.byteLength, 0);
}

function checkSize(total: number, limit: number): void {
  if (total > limit) {
    throw storeError("storage_limit", "The files exceed the browser storage budget.", { limit });
  }
}

function checkInputs(files: StoredInput[], limit: number): void {
  if (
    new Set(files.map((file) => file.id)).size !== files.length ||
    files.some((file) => !file.id)
  ) {
    throw storeError("invalid_input", "Input slot IDs must be nonempty and unique.");
  }
  checkSize(totalBytes(files), limit);
}

export interface FileStore {
  readonly inputLimitBytes: number;
  putInputs(files: StoredInput[]): Promise<void>;
  listInputs(): InputSlot[];
  readInputSlot(id: string, maxBytes?: number): Promise<Uint8Array>;
  readInputPrefix(maxBytes: number): Promise<Uint8Array>;
  putOutputs(artifacts: StoredArtifact[]): Promise<OutputLocation[]>;
  outputLocation(): OutputLocation | null;
  readonly kind: StorageKind;
  /** Stores the input, replacing any previous one, and returns its size. */
  putInput(file: Blob): Promise<number>;
  /** Reads the input, or its first `maxBytes` bytes. */
  readInput(maxBytes?: number): Promise<Uint8Array>;
  /** Stores the output, replacing any previous one. */
  putOutput(bytes: Uint8Array, mediaType: string): Promise<OutputLocation>;
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
  #inputs = new Map<string, { slot: InputSlot; blob: Blob }>();
  #outputs: OutputLocation[] = [];

  get inputLimitBytes(): number {
    return Math.min(this.limitBytes, STORE_INPUT_LIMIT_BYTES);
  }

  async putInputs(files: StoredInput[]): Promise<void> {
    checkInputs(files, this.inputLimitBytes);
    this.#inputs = new Map(
      files.map((file) => [
        file.id,
        {
          slot: { id: file.id, name: file.name, size: file.bytes.byteLength },
          blob: new Blob([file.bytes as Uint8Array<ArrayBuffer>]),
        },
      ]),
    );
  }

  listInputs(): InputSlot[] {
    return [...this.#inputs.values()].map(({ slot }) => ({ ...slot }));
  }

  async readInputSlot(id: string, maxBytes?: number): Promise<Uint8Array> {
    const input = this.#inputs.get(id);
    if (!input) throw noInput();
    return new Uint8Array(await input.blob.slice(0, maxBytes).arrayBuffer());
  }

  readInputPrefix(maxBytes: number): Promise<Uint8Array> {
    return this.readInput(maxBytes);
  }

  outputLocation(): OutputLocation | null {
    return this.#outputs[0] ?? null;
  }

  async putOutputs(artifacts: StoredArtifact[]): Promise<OutputLocation[]> {
    checkSize(totalBytes(artifacts), this.limitBytes);
    this.#outputs = artifacts.map((artifact) => ({
      kind: "memory",
      blob: new Blob([artifact.bytes as Uint8Array<ArrayBuffer>], { type: artifact.mediaType }),
    }));
    return [...this.#outputs];
  }

  constructor(readonly limitBytes = MEMORY_LIMIT_BYTES) {}

  async putInput(file: Blob): Promise<number> {
    if (file.size > this.limitBytes) {
      throw storeError(
        "storage_limit",
        `This browser cannot store files on the device, so inputs above ${Math.round(this.limitBytes / 1024 / 1024)} MB are not supported here.`,
        { limit: this.limitBytes },
      );
    }
    this.#inputs = new Map([
      [
        INPUT_NAME,
        {
          slot: {
            id: INPUT_NAME,
            name: file instanceof File ? file.name : INPUT_NAME,
            size: file.size,
          },
          blob: file,
        },
      ],
    ]);
    return file.size;
  }

  async readInput(maxBytes?: number): Promise<Uint8Array> {
    const id = this.#inputs.keys().next().value;
    if (!id) throw noInput();
    return this.readInputSlot(id, maxBytes);
  }

  async putOutput(bytes: Uint8Array, mediaType: string): Promise<OutputLocation> {
    const [location] = await this.putOutputs([{ name: OUTPUT_NAME, bytes, mediaType }]);
    return location as OutputLocation;
  }

  async clear(): Promise<void> {
    this.#inputs.clear();
    this.#outputs = [];
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
const OUTPUT_NAME = "output";

/**
 * Generates a random session ID.
 *
 * Uses `crypto.randomUUID()` when available in secure contexts, or derives an RFC 4122
 * version 4 UUID from `crypto.getRandomValues()` so workers can start on non-secure
 * origins (such as plain HTTP on a LAN) and fall back to memory storage.
 */
export function randomSessionId(cryptoInstance: Crypto = crypto): string {
  if (typeof cryptoInstance !== "undefined" && typeof cryptoInstance.randomUUID === "function") {
    return cryptoInstance.randomUUID();
  }
  const bytes = new Uint8Array(16);
  cryptoInstance.getRandomValues(bytes);
  const b6 = bytes[6];
  const b8 = bytes[8];
  if (b6 !== undefined && b8 !== undefined) {
    bytes[6] = (b6 & 0x0f) | 0x40;
    bytes[8] = (b8 & 0x3f) | 0x80;
  }
  const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

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
export function holdLock(locks: LockManager, session: string): Promise<void> {
  return new Promise((held, reject) => {
    locks
      .request(sessionLockName(session), () => {
        held();
        return new Promise<never>(() => {});
      })
      .catch(reject);
  });
}

export class OpfsStore implements FileStore {
  readonly kind = "opfs";
  readonly inputLimitBytes = STORE_INPUT_LIMIT_BYTES;
  #inputs = new Map<string, InputSlot & { file: string }>();
  #outputs: OutputLocation[] = [];

  listInputs(): InputSlot[] {
    return [...this.#inputs.values()].map(({ id, name, size }) => ({ id, name, size }));
  }

  outputLocation(): OutputLocation | null {
    return this.#outputs[0] ?? null;
  }

  readInputPrefix(maxBytes: number): Promise<Uint8Array> {
    return this.readInput(maxBytes);
  }

  async putInputs(files: StoredInput[]): Promise<void> {
    checkInputs(files, this.inputLimitBytes);
    const staged = new Map<string, InputSlot & { file: string }>();
    const directory = await this.directory("input");
    try {
      for (const input of files) {
        const file = `input-${randomSessionId()}`;
        staged.set(input.id, {
          id: input.id,
          name: input.name,
          size: input.bytes.byteLength,
          file,
        });
        await this.writeBytes("input", file, input.bytes);
      }
    } catch (error) {
      for (const { file } of staged.values()) await removeEntry(directory, file);
      throw error;
    }
    const previous = this.#inputs;
    this.#inputs = staged;
    for (const { file } of previous.values()) await removeEntry(directory, file);
  }

  private async writeBytes(
    kind: "input" | "output",
    file: string,
    bytes: Uint8Array | Blob,
  ): Promise<void> {
    const access = await this.access(kind, file, true);
    try {
      access.truncate(0);
      let offset = 0;
      const write = (chunk: Uint8Array) => {
        let consumed = 0;
        while (consumed < chunk.byteLength) {
          const written = access.write(chunk.subarray(consumed), { at: offset });
          if (written <= 0)
            throw storeError("storage_error", "The browser could not finish writing the file.");
          consumed += written;
          offset += written;
        }
      };
      if (bytes instanceof Blob) {
        const reader = bytes.stream().getReader();
        try {
          for (;;) {
            const chunk = await reader.read();
            if (chunk.done) break;
            write(chunk.value);
          }
        } finally {
          reader.releaseLock();
        }
      } else write(bytes);
      access.flush();
    } finally {
      access.close();
    }
  }

  async readInputSlot(id: string, maxBytes?: number): Promise<Uint8Array> {
    const input = this.#inputs.get(id);
    if (!input) throw noInput();
    const access = await this.access("input", input.file, false);
    try {
      const size = access.getSize();
      const buffer = new Uint8Array(maxBytes === undefined ? size : Math.min(size, maxBytes));
      let offset = 0;
      while (offset < buffer.byteLength) {
        const read = access.read(buffer.subarray(offset), { at: offset });
        if (read <= 0)
          throw storeError("storage_error", "The browser could not finish reading the file.");
        offset += read;
      }
      return buffer;
    } finally {
      access.close();
    }
  }

  async putOutputs(artifacts: StoredArtifact[]): Promise<OutputLocation[]> {
    checkSize(totalBytes(artifacts), this.inputLimitBytes);
    const directory = await this.directory("output");
    const staged: OutputLocation[] = [];
    try {
      for (const artifact of artifacts) {
        // Engine names are download labels, never OPFS path components.
        const name = `output-${randomSessionId()}`;
        staged.push({ kind: "opfs", directory: [STORE_DIRECTORY, this.session, "output"], name });
        await this.writeBytes("output", name, artifact.bytes);
      }
    } catch (error) {
      for (const location of staged)
        if (location.kind === "opfs") await removeEntry(directory, location.name);
      throw error;
    }
    const previous = this.#outputs;
    this.#outputs = staged;
    for (const location of previous)
      if (location.kind === "opfs") await removeEntry(directory, location.name);
    return [...staged];
  }

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
    checkSize(file.size, this.inputLimitBytes);
    const storedName = `input-${randomSessionId()}`;
    const directory = await this.directory("input");
    try {
      await this.writeBytes("input", storedName, file);
    } catch (error) {
      await removeEntry(directory, storedName);
      throw error;
    }
    const previous = this.#inputs;
    this.#inputs = new Map([
      [
        INPUT_NAME,
        {
          id: INPUT_NAME,
          name: file instanceof File ? file.name : INPUT_NAME,
          size: file.size,
          file: storedName,
        },
      ],
    ]);
    for (const { file: name } of previous.values()) await removeEntry(directory, name);
    return file.size;
  }

  async readInput(maxBytes?: number): Promise<Uint8Array> {
    const id = this.#inputs.keys().next().value;
    if (!id) throw noInput();
    return this.readInputSlot(id, maxBytes);
  }

  async putOutput(
    bytes: Uint8Array,
    mediaType = "application/octet-stream",
  ): Promise<OutputLocation> {
    const [location] = await this.putOutputs([{ name: OUTPUT_NAME, bytes, mediaType }]);
    return location as OutputLocation;
  }

  async clear(): Promise<void> {
    await removeEntry(await this.base(), this.session);
    this.#inputs.clear();
    this.#outputs = [];
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
