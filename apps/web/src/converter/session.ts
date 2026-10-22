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

/** Inputs above this size are classified but not analysed, as in the CLI. */
export const ANALYSIS_LIMIT_BYTES = 32 * 1024 * 1024;

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
  ) {}

  get storage() {
    return this.store.kind;
  }

  async ingest(file: File): Promise<IngestResult> {
    this.#input = null;
    await this.store.clear();
    const size = await this.store.putInput(file);
    const prefix = await this.store.readInput(ANALYSIS_LIMIT_BYTES + 1);
    const inspection = this.core.inspect(prefix, file.name, size);
    this.#input = { name: file.name, size, format: inspection.format };
    return { name: file.name, size, inspection };
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
    const output = await runRoute(this.core, bytes, route.steps, { profile, title: stem });
    const elapsedMs = performance.now() - started;
    const name = `${stem}.${output.extension}`;
    const location = await this.store.putOutput(name, output.bytes, output.mediaType);
    return {
      name,
      size: output.bytes.byteLength,
      format: to,
      mediaType: output.mediaType,
      route: route.steps,
      warnings: output.warnings,
      elapsedMs,
      location,
    };
  }

  async reset(): Promise<null> {
    this.#input = null;
    await this.store.clear();
    return null;
  }

  #requireInput(): CurrentInput {
    if (!this.#input) {
      throw { code: "no_input", message: "Choose a file first." } satisfies ConverterError;
    }
    return this.#input;
  }
}

/** Turns anything thrown into a ConverterError; unknown failures retire the worker. */
export function toConverterError(error: unknown): ConverterError {
  if (isConverterError(error)) {
    return error;
  }
  const message = error instanceof Error ? error.message : String(error);
  return { code: "internal_error", message: `The converter stopped: ${message}`, fatal: true };
}

export interface WorkerContext {
  session: ConverterSession;
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
    const { session, version, memoryLimitBytes, resources } = await context;
    let result: ResultMap[keyof ResultMap];
    switch (request.type) {
      case "init":
        result = { version, storage: session.storage, memoryLimitBytes, resources: resources() };
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
    }
    return { id: request.id, ok: true, result };
  } catch (error) {
    return { id: request.id, ok: false, error: toConverterError(error) };
  }
}
