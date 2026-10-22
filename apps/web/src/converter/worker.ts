// Web Worker that owns ariad-wasm and the converter's files.
import init, * as ariadWasm from "@ariadshift/ariad-wasm";
import { coreFrom } from "./engines";
import { MEMORY_LIMIT_BYTES, MemoryStore, OpfsStore } from "./file-store";
import type { RequestEnvelope, WorkerResponse } from "./protocol";
import { ConverterSession, handleRequest, type WorkerContext } from "./session";

const context: Promise<WorkerContext> = (async () => {
  await init();
  const store = (await OpfsStore.open()) ?? new MemoryStore();
  return {
    session: new ConverterSession(coreFrom(ariadWasm), store),
    version: ariadWasm.version(),
    memoryLimitBytes: MEMORY_LIMIT_BYTES,
    resources: () => [
      self.location.href,
      ...performance.getEntriesByType("resource").map((entry) => entry.name),
    ],
  };
})();

self.addEventListener("message", async (event: MessageEvent<RequestEnvelope>) => {
  const response: WorkerResponse = await handleRequest(context, event.data);
  self.postMessage(response);
});
