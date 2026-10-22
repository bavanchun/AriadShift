// Main-thread side of the converter worker: promise-based requests and downloads.
import { STORE_DIRECTORY } from "./file-store";
import type {
  ConverterError,
  OutputLocation,
  RequestEnvelope,
  ResultMap,
  WorkerRequest,
  WorkerResponse,
} from "./protocol";

type Pending = { resolve: (value: unknown) => void; reject: (error: ConverterError) => void };

export class ConverterClient {
  #worker: Worker | null = null;
  #next = 1;
  #pending = new Map<number, Pending>();

  constructor(private readonly spawn: () => Worker = spawnConverterWorker) {}

  request<K extends WorkerRequest["type"]>(
    message: Extract<WorkerRequest, { type: K }>,
  ): Promise<ResultMap[K]> {
    const worker = this.#ensure();
    const id = this.#next++;
    return new Promise<ResultMap[K]>((resolve, reject) => {
      this.#pending.set(id, { resolve: resolve as (value: unknown) => void, reject });
      worker.postMessage({ ...message, id } as RequestEnvelope);
    });
  }

  terminate() {
    this.#worker?.terminate();
    this.#worker = null;
    this.#failAll({ code: "worker_stopped", message: "The converter was stopped.", fatal: true });
  }

  #ensure(): Worker {
    if (this.#worker) {
      return this.#worker;
    }
    const worker = this.spawn();
    worker.addEventListener("message", (event: MessageEvent<WorkerResponse>) => {
      const response = event.data;
      const pending = this.#pending.get(response.id);
      if (!pending) {
        return;
      }
      this.#pending.delete(response.id);
      if (response.ok) {
        pending.resolve(response.result);
      } else {
        if (response.error.fatal) {
          this.#worker?.terminate();
          this.#worker = null;
        }
        pending.reject(response.error);
      }
    });
    worker.addEventListener("error", (event) => {
      event.preventDefault();
      this.#worker?.terminate();
      this.#worker = null;
      this.#failAll({
        code: "worker_failed",
        message: "The converter could not start in this browser.",
        fatal: true,
      });
    });
    this.#worker = worker;
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

export { STORE_DIRECTORY };
