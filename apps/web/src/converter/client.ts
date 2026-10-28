// Main-thread side of the converter worker: promise-based requests and downloads.
import type {
  ConverterError,
  OutputLocation,
  RequestEnvelope,
  ResultMap,
  WorkerRequest,
  WorkerResponse,
} from "./protocol";

type Pending = { resolve: (value: unknown) => void; reject: (error: ConverterError) => void };

/**
 * Talks to the converter worker. When it replaces a worker (after a fatal error, or
 * because the wasm module grew large) it asks the next worker to delete the old
 * worker's files, so nothing is left behind in the browser's storage.
 */
export class ConverterClient {
  #worker: Worker | null = null;
  #progress = new Set<(message: string) => void>();
  onProgress(listener: (message: string) => void): () => void {
    this.#progress.add(listener);
    return () => {
      this.#progress.delete(listener);
    };
  }

  /** Storage session of the current worker, known once its init request answered. */
  #session: string | null = null;
  #retired: string[] = [];
  #recycle = false;
  #next = 1;
  #pending = new Map<number, Pending>();

  constructor(private readonly spawn: () => Worker = spawnConverterWorker) {}

  request<K extends WorkerRequest["type"]>(
    message: Extract<WorkerRequest, { type: K }>,
  ): Promise<ResultMap[K]> {
    if (this.#recycle && (message.type === "ingest" || message.type === "reset")) {
      this.#retire();
    }
    const fresh = this.#worker === null;
    const worker = this.#ensure();
    if (fresh && this.#retired.length > 0) {
      const sessions = [...this.#retired];
      void this.#send(worker, { type: "retire", sessions }).then(
        () => {
          this.#retired = this.#retired.filter((session) => !sessions.includes(session));
        },
        () => undefined,
      );
    }
    return this.#send(worker, message).then((result) => {
      if (typeof result === "object" && result !== null && "recycle" in result && result.recycle) {
        this.#recycle = true;
      }
      return result;
    });
  }

  /** Stop the current conversion; the next worker cleans up its storage. */
  cancel() {
    this.#retire({ code: "cancelled", message: "The conversion was cancelled.", fatal: false });
  }

  terminate() {
    this.#worker?.terminate();
    this.#worker = null;
    this.#session = null;
    this.#failAll({ code: "worker_stopped", message: "The converter was stopped.", fatal: true });
  }

  #send<K extends WorkerRequest["type"]>(
    worker: Worker,
    message: Extract<WorkerRequest, { type: K }>,
  ): Promise<ResultMap[K]> {
    const id = this.#next++;
    return new Promise<ResultMap[K]>((resolve, reject) => {
      this.#pending.set(id, { resolve: resolve as (value: unknown) => void, reject });
      worker.postMessage({ ...message, id } as RequestEnvelope);
    });
  }

  /** Replaces the worker and remembers its storage session for cleanup. */
  #retire(
    error: ConverterError = {
      code: "worker_replaced",
      message: "The converter worker was replaced.",
      fatal: false,
    },
  ) {
    this.#worker?.terminate();
    if (this.#session) {
      this.#retired.push(this.#session);
    }
    this.#worker = null;
    this.#session = null;
    this.#recycle = false;
    this.#failAll(error);
  }

  #ensure(): Worker {
    if (this.#worker) {
      return this.#worker;
    }
    const worker = this.spawn();
    worker.addEventListener(
      "message",
      (event: MessageEvent<WorkerResponse | { event: "progress"; message: string }>) => {
        if (this.#worker !== worker) return;
        const response = event.data;
        if ("event" in response) {
          for (const listener of this.#progress) listener(response.message);
          return;
        }
        const pending = this.#pending.get(response.id);
        if (!pending) {
          return;
        }
        this.#pending.delete(response.id);
        if (response.ok) {
          pending.resolve(response.result);
        } else {
          pending.reject(response.error);
          if (response.error.fatal && this.#worker === worker) {
            this.#retire(response.error);
          }
        }
      },
    );
    worker.addEventListener("error", (event) => {
      event.preventDefault();
      if (this.#worker === worker) {
        this.#retire({
          code: "worker_failed",
          message: "The converter could not start in this browser.",
          fatal: true,
        });
      }
    });
    this.#worker = worker;
    this.#send(worker, { type: "init" }).then(
      (info) => {
        if (this.#worker === worker) {
          this.#session = info.session;
        }
      },
      () => undefined,
    );
    return worker;
  }

  #failAll(error: ConverterError) {
    for (const pending of this.#pending.values()) {
      pending.reject(error);
    }
    this.#pending.clear();
  }
}

export function spawnConverterWorker(): Worker {
  return new Worker(new URL("./worker.ts", import.meta.url), {
    type: "module",
    name: "ariadshift-converter",
  });
}

/** Opens a converted file without copying it into page state. */
export async function openOutput(location: OutputLocation): Promise<Blob> {
  if (location.kind === "memory") {
    return location.blob;
  }
  let directory = await navigator.storage.getDirectory();
  for (const part of location.directory) {
    directory = await directory.getDirectoryHandle(part);
  }
  return (await directory.getFileHandle(location.name)).getFile();
}

/** Saves a converted file through a temporary object URL. */
export async function downloadOutput(location: OutputLocation, name: string): Promise<void> {
  const blob = await openOutput(location);
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = name;
  link.rel = "noopener";
  document.body.append(link);
  link.click();
  link.remove();
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}
