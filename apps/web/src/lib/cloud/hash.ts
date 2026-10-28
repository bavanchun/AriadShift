// Hashes the picked file for `createUpload` without holding it in memory. A
// Web Worker reads it in 8 MiB slices into the SDK's incremental SHA-256, so
// hashing a 2 GiB file leaves the page responsive. Where a worker cannot start
// (tests, a blocked script), the same incremental hash runs on the main thread
// and yields between slices.
import { type HashOptions, sha256Hex } from "@ariadshift/sdk";

/** Posted to the hash worker: the file itself, which is shared, not copied. */
export interface HashRequest {
  file: Blob;
}

/** Posted back by the hash worker. */
export type HashResponse =
  | { kind: "progress"; hashed: number; total: number }
  | { kind: "done"; sha256: string }
  | { kind: "error"; name: string; message: string };

export type FileHashOptions = Pick<HashOptions, "onProgress" | "signal">;

function spawnHashWorker(): Worker | null {
  if (typeof Worker === "undefined") {
    return null;
  }
  try {
    return new Worker(new URL("./hash-worker.ts", import.meta.url), {
      type: "module",
      name: "ariadshift-hash",
    });
  } catch {
    return null;
  }
}

/** Hashes in the worker; resolves `null` when the worker itself fails to run. */
function hashInWorker(
  worker: Worker,
  file: Blob,
  { onProgress, signal }: FileHashOptions,
): Promise<string | null> {
  return new Promise((resolve, reject) => {
    const finish = () => {
      worker.terminate();
      signal?.removeEventListener("abort", onAbort);
    };
    const onAbort = () => {
      finish();
      reject(signal?.reason);
    };
    worker.addEventListener("message", (event: MessageEvent<HashResponse>) => {
      const message = event.data;
      if (message.kind === "progress") {
        onProgress?.(message.hashed, message.total);
        return;
      }
      finish();
      if (message.kind === "done") {
        resolve(message.sha256);
      } else {
        reject(Object.assign(new Error(message.message), { name: message.name }));
      }
    });
    worker.addEventListener("error", (event) => {
      // The worker script did not load or crashed: hash on the main thread instead.
      event.preventDefault();
      finish();
      resolve(null);
    });
    signal?.addEventListener("abort", onAbort, { once: true });
    worker.postMessage({ file } satisfies HashRequest);
  });
}

/** Lowercase hex SHA-256 of `file`, read in slices and reported as it goes. */
export async function hashFile(file: Blob, options: FileHashOptions = {}): Promise<string> {
  options.signal?.throwIfAborted();
  const worker = spawnHashWorker();
  if (worker) {
    const digest = await hashInWorker(worker, file, options);
    if (digest !== null) {
      return digest;
    }
  }
  return sha256Hex(file, options);
}
