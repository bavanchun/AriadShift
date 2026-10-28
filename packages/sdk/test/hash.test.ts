import { createHash } from "node:crypto";
import { afterEach, describe, expect, it, vi } from "vitest";
import { HASH_CHUNK_BYTES, sha256Hex } from "../src/index";

// FIPS 180-2 test vectors.
const VECTORS: Array<[string, string]> = [
  ["", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"],
  ["abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"],
  [
    "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
    "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
  ],
];
const MILLION_A = "a".repeat(1_000_000);
const MILLION_A_SHA256 = "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0";

/** Deterministic, non-repeating bytes, so a misordered chunk changes the digest. */
function pseudoRandomBytes(length: number): Uint8Array<ArrayBuffer> {
  const bytes = new Uint8Array(length);
  let state = 0x9e3779b9;
  for (let index = 0; index < length; index += 1) {
    state ^= state << 13;
    state ^= state >>> 17;
    state ^= state << 5;
    bytes[index] = state & 0xff;
  }
  return bytes;
}

const nodeSha256 = (bytes: Uint8Array) => createHash("sha256").update(bytes).digest("hex");

afterEach(() => {
  vi.restoreAllMocks();
});

describe("sha256Hex", () => {
  it.each(VECTORS)("matches the known digest of %j", async (text, digest) => {
    const bytes = new TextEncoder().encode(text);
    expect(await sha256Hex(new Blob([text]))).toBe(digest);
    expect(await sha256Hex(bytes)).toBe(digest);
    expect(await sha256Hex(bytes.slice().buffer)).toBe(digest);
  });

  it("hashes a multi-chunk input to the known digest", async () => {
    // 1 000 000 bytes in 65 000-byte chunks: 16 chunks, the last one partial and
    // every boundary off the 64-byte block size.
    const progress: number[] = [];
    const digest = await sha256Hex(new Blob([MILLION_A]), {
      chunkBytes: 65_000,
      onProgress: (hashed, total) => {
        expect(total).toBe(1_000_000);
        progress.push(hashed);
      },
    });

    expect(digest).toBe(MILLION_A_SHA256);
    expect(progress).toHaveLength(16);
    expect(progress[0]).toBe(65_000);
    expect(progress.at(-1)).toBe(1_000_000);
  });

  it("hashes across the default chunk size like a one-shot digest", async () => {
    const bytes = pseudoRandomBytes(HASH_CHUNK_BYTES * 2 + 12_345);
    const expected = nodeSha256(bytes);
    const progress: number[] = [];

    const digest = await sha256Hex(new Blob([bytes]), {
      onProgress: (hashed) => progress.push(hashed),
    });

    expect(digest).toBe(expected);
    expect(progress).toEqual([HASH_CHUNK_BYTES, HASH_CHUNK_BYTES * 2, bytes.length]);
    expect(await sha256Hex(bytes)).toBe(expected);
  });

  it("hashes a view into a larger buffer by the view only", async () => {
    const backing = pseudoRandomBytes(4_096);
    const view = backing.subarray(100, 3_000);
    expect(await sha256Hex(view, { chunkBytes: 1_000 })).toBe(nodeSha256(view));
  });

  it("reads a Blob one slice at a time, never whole", async () => {
    const bytes = pseudoRandomBytes(250_000);
    const blob = new Blob([bytes]);
    const read = vi.spyOn(Blob.prototype, "arrayBuffer");

    expect(await sha256Hex(blob, { chunkBytes: 100_000 })).toBe(nodeSha256(bytes));
    const sizes = read.mock.contexts.map((context) => (context as Blob).size);
    expect(sizes).toEqual([100_000, 100_000, 50_000]);
  });

  it("reports nothing for an empty body", async () => {
    const onProgress = vi.fn();
    expect(await sha256Hex(new Blob([]), { onProgress })).toBe(VECTORS[0]?.[1]);
    expect(onProgress).not.toHaveBeenCalled();
  });

  it("stops between chunks when the signal aborts", async () => {
    const controller = new AbortController();
    const reason = new DOMException("stopped", "AbortError");
    const onProgress = vi.fn((hashed: number) => {
      if (hashed === 1_000) {
        controller.abort(reason);
      }
    });

    await expect(
      sha256Hex(new Blob([MILLION_A]), {
        chunkBytes: 1_000,
        signal: controller.signal,
        onProgress,
      }),
    ).rejects.toBe(reason);
    expect(onProgress).toHaveBeenCalledTimes(1);
  });

  it("rejects a chunk size that is not a positive integer", async () => {
    await expect(sha256Hex(new Blob(["abc"]), { chunkBytes: 0 })).rejects.toThrow(RangeError);
    await expect(sha256Hex(new Blob(["abc"]), { chunkBytes: 1.5 })).rejects.toThrow(RangeError);
  });
});
