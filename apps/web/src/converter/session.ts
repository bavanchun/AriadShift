// Worker-side converter state, independent of worker globals so tests can drive it.
import type { FormatId, InspectResult, ProfileId } from "@ariadshift/ariad-wasm";
import { type Core, runRoute } from "./engines";
import { type FileStore, randomSessionId, STORE_INPUT_LIMIT_BYTES } from "./file-store";
import { inspectPdfInsights } from "./insights";
import {
  type ConvertedFile,
  type ConverterError,
  type IngestResult,
  isConverterError,
  type RequestEnvelope,
  type ResultMap,
  type ToolExecutionResult,
  type WorkerRequest,
  type WorkerResponse,
} from "./protocol";
import { toolCatalog } from "./tool-catalog";

/**
 * Largest input the browser converter accepts. Reading a document expands it in wasm
 * memory by up to roughly 70 times (HTML), and that memory is only returned when the
 * worker is replaced; larger files belong to the CLI or the desktop app.
 */
export const INPUT_LIMIT_BYTES = STORE_INPUT_LIMIT_BYTES;
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
      let details = {};
      try {
        const prefix = new Uint8Array(await file.slice(0, CLASSIFY_PREFIX_BYTES).arrayBuffer());
        const inspection = this.core.inspect(prefix, file.name, file.size);
        details = {
          format: inspection.format,
          targets: [...inspection.reachable, ...inspection.reachable_elsewhere],
        };
      } catch {
        /* The size error remains useful even for an unknown format. */
      }

      throw {
        code: "limit_exceeded",
        limit: "browser_input_bytes",
        ...details,
        message: `${file.name} is ${Math.ceil(file.size / MB)} MB. The browser converter takes files up to ${this.inputLimitBytes / MB} MB; the AriadShift CLI converts larger ones on your computer.`,
      } satisfies ConverterError;
    }
    const size = await this.store.putInput(file);
    const sample = await this.store.readInput(
      size > ANALYSIS_LIMIT_BYTES ? CLASSIFY_PREFIX_BYTES : undefined,
    );
    try {
      const inspection = this.core.inspect(sample, file.name, size);
      this.#input = { name: file.name, size, format: inspection.format };
      return { name: file.name, size, inspection, recycle: this.#oversized() };
    } catch (error) {
      await this.store.clear();
      throw error;
    }
  }

  /** Replaces the current input set. Returned IDs determine tool input order. */
  async ingestMany(files: File[]): Promise<ResultMap["ingestMany"]> {
    const total = files.reduce((sum, file) => sum + file.size, 0);
    const limit = Math.min(this.inputLimitBytes, this.store.inputLimitBytes);
    if (total > limit)
      throw {
        code: "limit_exceeded",
        limit: "browser_input_bytes",
        message: `The input files exceed the ${limit} byte browser budget.`,
      } satisfies ConverterError;
    const inputs = [];
    const results: ResultMap["ingestMany"] = [];
    for (const file of files) {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const id = randomSessionId();
      let inspection: InspectResult | null = null;
      let inspectionError: ConverterError | undefined;
      try {
        inspection = this.core.inspect(
          bytes.byteLength > ANALYSIS_LIMIT_BYTES
            ? bytes.subarray(0, CLASSIFY_PREFIX_BYTES)
            : bytes,
          file.name,
          file.size,
        );
      } catch (error) {
        if (!isConverterError(error) || error.fatal) throw error;
        inspectionError = error;
      }
      inputs.push({ id, name: file.name, bytes });
      results.push({
        id,
        name: file.name,
        size: file.size,
        inspection,
        ...(inspectionError ? { inspectionError } : {}),
        recycle: this.#oversized(),
      });
    }
    await this.store.putInputs(inputs);
    const first = results[0];
    this.#input = first?.inspection
      ? { name: first.name, size: first.size, format: first.inspection.format }
      : null;
    return results;
  }

  listTools() {
    return toolCatalog(this.core.listTools?.());
  }

  async runTool(
    request: Extract<WorkerRequest, { type: "runTool" }>,
  ): Promise<ToolExecutionResult> {
    const tool = this.listTools().find((candidate) => candidate.id === request.toolId);
    if (!tool)
      throw {
        code: "unknown_tool",
        message: `Unknown tool: ${request.toolId}`,
      } satisfies ConverterError;
    if (!tool.runtimes.includes("wasm") || !this.core.runTool)
      throw {
        code: "engine_unavailable",
        message: "Run this tool on your computer or through its supported cloud service.",
      } satisfies ConverterError;
    const ids = request.inputIds ?? this.store.listInputs().map((slot) => slot.id);
    const slots = new Map(this.store.listInputs().map((slot) => [slot.id, slot]));
    const selected = ids.map((id) => {
      const slot = slots.get(id);
      if (!slot)
        throw { code: "no_input", message: `Unknown input slot: ${id}` } satisfies ConverterError;
      return slot;
    });
    const budget =
      request.budgetBytes ?? Math.min(this.inputLimitBytes, this.store.inputLimitBytes);
    if (!Number.isSafeInteger(budget) || budget <= 0)
      throw {
        code: "invalid_params",
        message: "The input byte budget must be a positive safe integer.",
      } satisfies ConverterError;
    if (selected.reduce((sum, slot) => sum + slot.size, 0) > budget)
      throw {
        code: "limit_exceeded",
        message: "The selected inputs exceed the tool byte budget.",
      } satisfies ConverterError;
    const inputs = [];
    for (const slot of selected) inputs.push(await this.store.readInputSlot(slot.id));
    const started = performance.now();
    const result = this.core.runTool(
      inputs,
      selected.map((slot) => slot.name),
      tool.id,
      JSON.stringify(request.params),
      budget,
      request.entitled,
    );
    const artifacts = result.artifacts.length
      ? result.artifacts
      : [
          {
            name: `${fileStem(selected[0]?.name ?? "document")}.${result.extension}`,
            bytes: result.bytes,
            format: result.format,
            media_type: result.media_type,
            extension: result.extension,
          },
        ];
    const locations = await this.store.putOutputs(
      artifacts.map((artifact) => ({
        name: artifact.name,
        bytes: artifact.bytes,
        mediaType: artifact.media_type,
      })),
    );
    return {
      tool: result.tool,
      format: result.format,
      mediaType: result.media_type,
      extension: result.extension,
      primaryLocation: locations[0] as ResultMap["runTool"]["primaryLocation"],
      artifacts: artifacts.map((artifact, index) => ({
        name: artifact.name,
        size: artifact.bytes.byteLength,
        format: artifact.format,
        mediaType: artifact.media_type,
        extension: artifact.extension,
        location: locations[index] as ResultMap["runTool"]["primaryLocation"],
      })),
      data: result.data,
      warnings: result.warnings,
      metrics: result.metrics,
      elapsedMs: performance.now() - started,
      recycle: this.#oversized(),
    };
  }

  async inspectInsights(inputId?: string): Promise<ResultMap["inspectInsights"]> {
    const slot = this.store.listInputs().find((candidate) => !inputId || candidate.id === inputId);
    if (!slot) throw { code: "no_input", message: "Choose a PDF first." } satisfies ConverterError;
    return inspectPdfInsights(
      this.core,
      await this.store.readInputSlot(slot.id),
      slot.name,
      Math.min(this.inputLimitBytes, this.store.inputLimitBytes),
      this.listTools(),
    );
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
      case "ingestMany":
        result = await session.ingestMany(request.files);
        break;
      case "listTools":
        result = session.listTools();
        break;
      case "runTool":
        result = await session.runTool(request);
        break;
      case "inspectInsights":
        result = await session.inspectInsights(request.inputId);
        break;
      case "plan":
        result = session.plan(request.to, request.profile);
        break;
      case "convert":
        result = {
          ...(await session.convert(request.to, request.profile)),
          resources: resources(),
        };
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
