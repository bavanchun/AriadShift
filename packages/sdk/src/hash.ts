// Incremental SHA-256 of upload bodies. Web Crypto only digests a whole
// buffer, which would hold a 2 GiB upload in memory at once; this reads a Blob
// one slice at a time into an incremental hash, so memory stays near one chunk
// whatever the file size, and reports progress after each chunk.
import { sha256 } from "@noble/hashes/sha2.js";
import { bytesToHex } from "@noble/hashes/utils.js";

/** Bytes read and hashed per step: 8 MiB. */
export const HASH_CHUNK_BYTES = 8 * 1024 * 1024;

/** Bodies the hash reads: Blobs (and Files) in slices, buffers in place. */
export type HashInput = Blob | ArrayBuffer | Uint8Array;

export interface HashOptions {
  /** Called after each chunk with the bytes hashed so far and the total. */
  onProgress?: (hashed: number, total: number) => void;
  /** Stops between chunks with the signal's reason. */
  signal?: AbortSignal;
  /** Bytes per chunk. Default {@link HASH_CHUNK_BYTES}. */
  chunkBytes?: number;
}

async function chunkAt(body: HashInput, start: number, end: number): Promise<Uint8Array> {
  if (body instanceof Blob) {
    return new Uint8Array(await body.slice(start, end).arrayBuffer());
  }
  return (body instanceof Uint8Array ? body : new Uint8Array(body)).subarray(start, end);
}

/** Lowercase hex SHA-256 of `body`, the digest format the API expects. */
export async function sha256Hex(body: HashInput, options: HashOptions = {}): Promise<string> {
  const { onProgress, signal } = options;
  const chunkBytes = options.chunkBytes ?? HASH_CHUNK_BYTES;
  if (!Number.isSafeInteger(chunkBytes) || chunkBytes <= 0) {
    throw new RangeError(`chunkBytes must be a positive integer, got ${chunkBytes}`);
  }
  const total = body instanceof Blob ? body.size : body.byteLength;
  const hash = sha256.create();
  for (let start = 0; start < total; start += chunkBytes) {
    signal?.throwIfAborted();
    const end = Math.min(start + chunkBytes, total);
    hash.update(await chunkAt(body, start, end));
    onProgress?.(end, total);
  }
  signal?.throwIfAborted();
  return bytesToHex(hash.digest());
}
