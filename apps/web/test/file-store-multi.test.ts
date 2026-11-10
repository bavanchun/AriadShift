import { openOutput } from "@/converter/client";
import {
  MEMORY_LIMIT_BYTES,
  MemoryStore,
  OpfsStore,
  STORE_INPUT_LIMIT_BYTES,
} from "@/converter/file-store";

const bytes = (value: string) => new TextEncoder().encode(value);

// In-memory implementation of the browser's synchronous file API exercises real OpfsStore paths.
class Directory {
  children = new Map<string, Directory | Uint8Array>();
  async getDirectoryHandle(name: string, options?: { create?: boolean }): Promise<Directory> {
    if (!this.children.has(name) && options?.create) this.children.set(name, new Directory());
    const value = this.children.get(name);
    if (!(value instanceof Directory)) throw new DOMException("Not found", "NotFoundError");
    return value;
  }
  async getFileHandle(name: string, options?: { create?: boolean }) {
    if (!this.children.has(name) && options?.create) this.children.set(name, new Uint8Array());
    if (!(this.children.get(name) instanceof Uint8Array))
      throw new DOMException("Not found", "NotFoundError");
    return {
      getFile: async () => new Blob([this.children.get(name) as Uint8Array<ArrayBuffer>]),
      createSyncAccessHandle: async () => ({
        close() {},
        flush() {},
        getSize: () => (this.children.get(name) as Uint8Array).byteLength,
        truncate: (size: number) => {
          this.children.set(name, new Uint8Array(size));
        },
        read: (buffer: Uint8Array, { at = 0 } = {}) => {
          buffer.set((this.children.get(name) as Uint8Array).subarray(at, at + buffer.length));
          return buffer.length;
        },
        write: (buffer: Uint8Array, { at = 0 } = {}) => {
          const previous = this.children.get(name) as Uint8Array;
          const next = new Uint8Array(Math.max(previous.length, at + buffer.length));
          next.set(previous);
          next.set(buffer, at);
          this.children.set(name, next);
          return buffer.length;
        },
      }),
    };
  }
  async removeEntry(name: string) {
    this.children.delete(name);
  }
  async *keys() {
    yield* this.children.keys();
  }
}

afterEach(() => vi.unstubAllGlobals());

async function opfs() {
  const root = new Directory();
  vi.stubGlobal("navigator", {
    storage: { getDirectory: async () => root },
    locks: {
      request: (_name: string, options: unknown, callback?: (lock: object) => unknown) =>
        typeof options === "function" ? options() : callback?.({}),
    },
  });
  vi.stubGlobal(
    "FileSystemFileHandle",
    class {
      createSyncAccessHandle() {}
    },
  );
  const store = await OpfsStore.open("test-multi");
  expect(store).not.toBeNull();
  return store as OpfsStore;
}

for (const [kind, create] of [
  ["memory", async () => new MemoryStore()],
  ["opfs", opfs],
] as const) {
  describe(`${kind} multi-file storage`, () => {
    it("roundtrips named slots and prefixes while preserving single-input operations", async () => {
      const store = await create();
      await store.putInputs([
        { id: "b", name: "../b.pdf", bytes: bytes("second") },
        { id: "a", name: "a.pdf", bytes: bytes("first") },
      ]);
      expect(store.listInputs().map((slot) => slot.id)).toEqual(["b", "a"]);
      expect(await store.readInputPrefix(3)).toEqual(bytes("sec"));
      expect(await store.readInputSlot("a")).toEqual(bytes("first"));
      await store.putInput(new File(["replacement"], "new.pdf"));
      expect(await store.readInput()).toEqual(bytes("replacement"));
      expect(store.listInputs()).toHaveLength(1);
      await expect(store.readInputSlot("a")).rejects.toMatchObject({ code: "no_input" });
    });

    it("keeps every output accessible and clears both inputs and outputs", async () => {
      const store = await create();
      const artifacts = ["../same.pdf", "../same.pdf"].map((name, index) => ({
        name,
        bytes: bytes(`part ${index}`),
        mediaType: "application/pdf",
      }));
      const locations = await store.putOutputs(artifacts);
      expect(locations).toHaveLength(2);
      for (const [index, location] of locations.entries())
        expect(await (await openOutput(location)).text()).toBe(`part ${index}`);
      expect(store.outputLocation()).toEqual(locations[0]);
      await store.putOutput(bytes("single"), "text/plain");
      expect(
        await (
          await openOutput(
            store.outputLocation() as NonNullable<ReturnType<typeof store.outputLocation>>,
          )
        ).text(),
      ).toBe("single");
      await store.clear();
      expect(store.outputLocation()).toBeNull();
      expect(store.listInputs()).toEqual([]);
    });

    it("stores single and multiple outputs above the input limit", async () => {
      const store = await create();
      const output = new Uint8Array(STORE_INPUT_LIMIT_BYTES + 1024);
      const location = await store.putOutput(output, "application/octet-stream");
      expect((await openOutput(location)).size).toBe(output.byteLength);

      const locations = await store.putOutputs([
        { name: "large.bin", bytes: output, mediaType: "application/octet-stream" },
        { name: "small.txt", bytes: bytes("x"), mediaType: "text/plain" },
      ]);
      expect(
        await Promise.all(locations.map(async (location) => (await openOutput(location)).size)),
      ).toEqual([output.byteLength, 1]);
    });

    it("rejects single and aggregate output overflow before replacing current outputs", async () => {
      const store = await create();
      const location = await store.putOutput(bytes("keep"), "text/plain");
      const error = { code: "storage_limit", limit: MEMORY_LIMIT_BYTES };
      await expect(
        store.putOutput(new Uint8Array(MEMORY_LIMIT_BYTES + 1), "application/octet-stream"),
      ).rejects.toMatchObject(error);
      await expect(
        store.putOutputs([
          {
            name: "large.bin",
            bytes: new Uint8Array(MEMORY_LIMIT_BYTES),
            mediaType: "application/octet-stream",
          },
          { name: "small.txt", bytes: bytes("x"), mediaType: "text/plain" },
        ]),
      ).rejects.toMatchObject(error);
      expect(store.outputLocation()).toEqual(location);
      expect(await (await openOutput(location)).text()).toBe("keep");
    });

    it("rejects aggregate overflow and duplicate IDs before replacing current inputs", async () => {
      const store = await create();
      await store.putInput(new Blob(["keep"]));
      await expect(
        store.putInputs([
          { id: "a", name: "a", bytes: new Uint8Array(STORE_INPUT_LIMIT_BYTES) },
          { id: "b", name: "b", bytes: bytes("x") },
        ]),
      ).rejects.toMatchObject({ code: "storage_limit" });
      await expect(
        store.putInputs([
          { id: "a", name: "a", bytes: bytes("a") },
          { id: "a", name: "b", bytes: bytes("b") },
        ]),
      ).rejects.toMatchObject({ code: "invalid_input" });
      expect(await store.readInput()).toEqual(bytes("keep"));
    });
  });
}

it("enforces the combined memory output budget", async () => {
  const store = new MemoryStore(5);
  await expect(
    store.putOutputs([
      { name: "a", bytes: bytes("123"), mediaType: "text/plain" },
      { name: "b", bytes: bytes("456"), mediaType: "text/plain" },
    ]),
  ).rejects.toMatchObject({ code: "storage_limit" });
});
