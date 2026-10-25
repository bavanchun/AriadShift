import { createHash } from "node:crypto";
import { compileEngine, ENGINE_CACHE, engineResponse } from "@/converter/wasm-cache";

const URL_KEY = "https://example.test/wasm/pandoc/pandoc.wasm?v=test";
// The smallest valid WebAssembly module: magic number and version, no sections.
const VALID = new Uint8Array([0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]);
const DAMAGED = new TextEncoder().encode("<!doctype html><title>Not found</title>");
const sha256 = (bytes: Uint8Array) => createHash("sha256").update(bytes).digest("hex");

/** In-memory Cache Storage holding one engine cache, keyed by URL. */
function fakeCaches() {
  const entries = new Map<string, Uint8Array>();
  const cache = {
    async match(url: string) {
      const bytes = entries.get(url);
      return bytes ? new Response(bytes.slice()) : undefined;
    },
    async put(url: string, response: Response) {
      entries.set(url, new Uint8Array(await response.arrayBuffer()));
    },
    async delete(url: string) {
      return entries.delete(url);
    },
  };
  const storage = {
    open: vi.fn(async (name: string) => {
      expect(name).toBe(ENGINE_CACHE);
      return cache;
    }),
  };
  return { entries, storage };
}

let store: ReturnType<typeof fakeCaches>;
let fetchMock: ReturnType<typeof vi.fn>;

beforeEach(() => {
  store = fakeCaches();
  fetchMock = vi.fn();
  vi.stubGlobal("caches", store.storage);
  vi.stubGlobal("fetch", fetchMock);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

async function bytesOf(response: Response) {
  return new Uint8Array(await response.arrayBuffer());
}

describe("engineResponse", () => {
  it("serves a verified cached binary without the network", async () => {
    store.entries.set(URL_KEY, VALID);
    const response = await engineResponse(URL_KEY, sha256(VALID));
    expect(await bytesOf(response)).toEqual(VALID);
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("evicts a cached binary with the wrong digest and caches a verified download", async () => {
    store.entries.set(URL_KEY, DAMAGED);
    fetchMock.mockResolvedValue(new Response(VALID.slice()));
    const response = await engineResponse(URL_KEY, sha256(VALID));
    expect(await bytesOf(response)).toEqual(VALID);
    expect(fetchMock).toHaveBeenCalledWith(URL_KEY);
    expect(store.entries.get(URL_KEY)).toEqual(VALID);
  });

  it("rejects a download with the wrong digest without caching it", async () => {
    fetchMock.mockResolvedValue(new Response(DAMAGED.slice()));
    await expect(engineResponse(URL_KEY, sha256(VALID))).rejects.toMatchObject({
      code: "engine_unavailable",
    });
    expect(store.entries.has(URL_KEY)).toBe(false);
  });

  it("drops a damaged cached copy even when the replacement download fails", async () => {
    store.entries.set(URL_KEY, DAMAGED);
    fetchMock.mockRejectedValue(new TypeError("offline"));
    await expect(engineResponse(URL_KEY, sha256(VALID))).rejects.toMatchObject({
      code: "engine_unavailable",
    });
    expect(store.entries.has(URL_KEY)).toBe(false);
  });

  it("returns a cached binary unchanged when no digest is pinned", async () => {
    store.entries.set(URL_KEY, DAMAGED);
    expect(await bytesOf(await engineResponse(URL_KEY))).toEqual(DAMAGED);
    expect(fetchMock).not.toHaveBeenCalled();
  });
});

describe("compileEngine", () => {
  it("compiles a verified binary", async () => {
    store.entries.set(URL_KEY, VALID);
    expect(await compileEngine(URL_KEY, sha256(VALID))).toBeInstanceOf(WebAssembly.Module);
  });

  it("evicts a cached binary that matches its digest but does not compile", async () => {
    store.entries.set(URL_KEY, DAMAGED);
    await expect(compileEngine(URL_KEY, sha256(DAMAGED))).rejects.toMatchObject({
      code: "engine_unavailable",
    });
    expect(store.entries.has(URL_KEY)).toBe(false);
  });
});
