// Worker-side converter state, independent of worker globals so tests can drive it.
import type { FormatId, ProfileId } from "@ariadshift/ariad-wasm";
import { type Core, runRoute } from "./engines";
import type { FileStore } from "./file-store";
import {
  type ConvertedFile,
  type ConverterError,
  type IngestResult,
  isConverterError,
  type RequestEnvelope,
  type ResultMap,
  type WorkerResponse,
} from "./protocol";

/**
 * Largest input the browser converter accepts. Reading a document expands it in wasm
 * memory by up to roughly 70 times (HTML), and that memory is only returned when the
 * worker is replaced; larger files belong to the CLI or the desktop app.
 */
export const INPUT_LIMIT_BYTES = 16 * 1024 * 1024;
/** Inputs up to this size are read in full for their structure counts. */
export const ANALYSIS_LIMIT_BYTES = 4 * 1024 * 1024;
/** Larger inputs are classified from this many leading bytes only. */
export const CLASSIFY_PREFIX_BYTES = 64 * 1024;
/** Above this wasm memory size the worker asks to be replaced before the next file. */
export const RECYCLE_MEMORY_BYTES = 256 * 1024 * 1024;

const MB = 1024 * 1024;

/** The file name without its extension; `.ir.json` counts as one extension. */
export function fileStem(name: string): string {
  const base = name.replace(/\.ir\.json$/i, "");
  if (base !== name) {
    return base || "document";
  }
  const dot = name.lastIndexOf(".");
  return (dot > 0 ? name.slice(0, dot) : name) || "document";
}

interface CurrentInput {
  name: string;
  size: number;
  format: FormatId;
}

export class ConverterSession {
  #input: CurrentInput | null = null;

  constructor(
    private readonly core: Core,
    private readonly store: FileStore,
    private readonly inputLimitBytes = INPUT_LIMIT_BYTES,
  ) {}

  get storage() {
    return this.store.kind;
  }

  async ingest(file: File): Promise<IngestResult> {
    this.#input = null;
    await this.store.clear();
    if (file.size > this.inputLimitBytes) {
      throw {
        code: "limit_exceeded",
        limit: "browser_input_bytes",
        message: `${file.name} is ${Math.ceil(file.size / MB)} MB. The browser converter takes files up to ${this.inputLimitBytes / MB} MB; the AriadShift CLI converts larger ones on your computer.`,
      } satisfies ConverterError;
    }
    const size = await this.store.putInput(file);
    const sample = await this.store.readInput(
      size > ANALYSIS_LIMIT_BYTES ? CLASSIFY_PREFIX_BYTES : undefined,
    );
    const inspection = this.core.inspect(sample, file.name, size);
    this.#input = { name: file.name, size, format: inspection.format };
    return { name: file.name, size, inspection, recycle: this.#oversized() };
  }

  plan(to: FormatId, profile: ProfileId) {
    const input = this.#requireInput();
    return this.core.plan(input.format, to, profile, "browser");
  }

  async convert(to: FormatId, profile: ProfileId): Promise<ConvertedFile> {
    const input = this.#requireInput();
    const route = this.core.plan(input.format, to, profile, "browser");
    const bytes = await this.store.readInput();
    const started = performance.now();
    const stem = fileStem(input.name);
    const output = await runRoute(this.core, bytes, route.steps, {
      profile,
      title: stem,
      limits: { max_input_bytes: this.inputLimitBytes },
    });
    const elapsedMs = performance.now() - started;
    const name = `${stem}.${output.extension}`;
    const location = await this.store.putOutput(output.bytes, output.mediaType);
    return {
      name,
      size: output.bytes.byteLength,
      format: to,
      mediaType: output.mediaType,
      route: route.steps,
      warnings: output.warnings,
      elapsedMs,
      location,
      recycle: this.#oversized(),
    };
  }

  async reset(): Promise<null> {
    this.#input = null;
    await this.store.clear();
    return null;
  }

  async retire(sessions: string[]): Promise<{ held: number }> {
    return { held: await this.store.retire(sessions) };
  }

  #oversized(): boolean {
    return this.core.memoryBytes() > RECYCLE_MEMORY_BYTES;
  }

  #requireInput(): CurrentInput {
    if (!this.#input) {
      throw { code: "no_input", message: "Choose a file first." } satisfies ConverterError;
    }
    return this.#input;
  }
}

/**
 * Turns anything thrown into a ConverterError. Storage failures are ordinary errors;
 * anything else (a trapped or exhausted wasm module) retires the worker.
 */
export function toConverterError(error: unknown): ConverterError {
  if (isConverterError(error)) {
    return error;
  }
  if (error instanceof DOMException) {
    return error.name === "QuotaExceededError"
      ? { code: "storage_full", message: "The browser has no room left to store this file." }
      : {
          code: "storage_error",
          message: `The browser's file storage failed (${error.name}). Start over and try again.`,
        };
  }
  const message = error instanceof Error ? error.message : String(error);
  return { code: "internal_error", message: `The converter stopped: ${message}`, fatal: true };
}

export interface WorkerContext {
  session: ConverterSession;
  storageSession: string;
  version: string;
  memoryLimitBytes: number;
  resources: () => string[];
}

/** Handles one request envelope and never throws. */
export async function handleRequest(
  context: Promise<WorkerContext>,
  request: RequestEnvelope,
): Promise<WorkerResponse> {
  try {
    const { session, storageSession, version, memoryLimitBytes, resources } = await context;
    let result: ResultMap[keyof ResultMap];
    switch (request.type) {
      case "init":
        result = {
          version,
          session: storageSession,
          storage: session.storage,
          memoryLimitBytes,
          inputLimitBytes: INPUT_LIMIT_BYTES,
          resources: resources(),
        };
        break;
      case "ingest":
        result = await session.ingest(request.file);
        break;
      case "plan":
        result = session.plan(request.to, request.profile);
        break;
      case "convert":
        result = await session.convert(request.to, request.profile);
        break;
      case "reset":
        result = await session.reset();
        break;
      case "retire":
        result = await session.retire(request.sessions);
        break;
    }
    return { id: request.id, ok: true, result };
  } catch (error) {
    return { id: request.id, ok: false, error: toConverterError(error) };
  }
}

/** Runs handlers one at a time, in arrival order, so file operations never overlap. */
export function serialize<T>(): (task: () => Promise<T>) => Promise<T> {
  let tail: Promise<unknown> = Promise.resolve();
  return (task) => {
    const run = tail.then(task, task);
    tail = run.catch(() => undefined);
    return run;
  };
}
