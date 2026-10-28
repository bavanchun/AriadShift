// Web Worker that hashes one file for a cloud upload, off the main thread.
import { sha256Hex } from "@ariadshift/sdk";
import type { HashRequest, HashResponse } from "./hash";

const post = (message: HashResponse) => self.postMessage(message);

self.addEventListener("message", (event: MessageEvent<HashRequest>) => {
  void sha256Hex(event.data.file, {
    onProgress: (hashed, total) => post({ kind: "progress", hashed, total }),
  }).then(
    (sha256) => post({ kind: "done", sha256 }),
    (error: unknown) =>
      post({
        kind: "error",
        name: error instanceof Error ? error.name : "Error",
        message: error instanceof Error ? error.message : String(error),
      }),
  );
});
