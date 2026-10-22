// Web Worker that owns ariad-wasm and the converter's files.
import init, * as ariadWasm from "@ariadshift/ariad-wasm";
import { coreFrom } from "./engines";
import { MEMORY_LIMIT_BYTES, MemoryStore, OpfsStore } from "./file-store";
import type { RequestEnvelope, WorkerResponse } from "./protocol";
import { ConverterSession, handleRequest, serialize, type WorkerContext } from "./session";

const storageSession = crypto.randomUUID();

const context: Promise<WorkerContext> = (async () => {
  const { memory } = await init();
  const store = (await OpfsStore.open(storageSession)) ?? new MemoryStore();
  return {
    session: new ConverterSession(coreFrom(ariadWasm, memory), store),
    storageSession,
    version: ariadWasm.version(),
    memoryLimitBytes: MEMORY_LIMIT_BYTES,
    resources: () => [
      self.location.href,
      ...performance.getEntriesByType("resource").map((entry) => entry.name),
    ],
  };
})();

const queue = serialize<WorkerResponse>();

self.addEventListener("message", (event: MessageEvent<RequestEnvelope>) => {
  void queue(() => handleRequest(context, event.data)).then((response) => {
    self.postMessage(response);
  });
});
