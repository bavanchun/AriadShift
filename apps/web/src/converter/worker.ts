// Web Worker that owns ariad-wasm, local tool analysis, and the converter's files.
import init, * as ariadWasm from "@ariadshift/ariad-wasm";
import { browserEngines, warmCachedEngines } from "./browser-engines";
import { coreFrom } from "./engines";
import { MEMORY_LIMIT_BYTES, MemoryStore, OpfsStore, randomSessionId } from "./file-store";
import type { RequestEnvelope, WorkerResponse } from "./protocol";
import { ConverterSession, handleRequest, serialize, type WorkerContext } from "./session";
import { usedEngines } from "./wasm-cache";

const storageSession = randomSessionId();

const context: Promise<WorkerContext> = (async () => {
  const { memory } = await init();
  const store = (await OpfsStore.open(storageSession)) ?? new MemoryStore();
  const engines = browserEngines(coreFrom(ariadWasm, memory));
  await warmCachedEngines();
  return {
    session: new ConverterSession(engines, store),
    storageSession,
    version: engines.version(),
    memoryLimitBytes: MEMORY_LIMIT_BYTES,
    // Engines read from Cache Storage leave no Resource Timing entry, so list them too.
    resources: () => [
      self.location.href,
      ...performance.getEntriesByType("resource").map((entry) => entry.name),
      ...usedEngines,
    ],
  };
})();

const queue = serialize<WorkerResponse>();

self.addEventListener("message", (event: MessageEvent<RequestEnvelope>) => {
  void queue(() => handleRequest(context, event.data)).then((response) => {
    self.postMessage(response);
  });
});
