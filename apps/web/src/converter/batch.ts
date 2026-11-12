import { ConverterClient, openOutput } from "./client";
import { type ConverterError, isConverterError, type ToolExecutionResult } from "./protocol";
import { toolCatalog } from "./tool-catalog";
import { createZipArchive, type ZipEntryInput } from "./zip";

export type BatchItemStatus = "queued" | "running" | "done" | "failed" | "cancelled";

export interface BatchItem {
  id: string;
  file: File;
  status: BatchItemStatus;
  /** Output locations are detached from the worker's resettable storage. */
  result?: ToolExecutionResult;
  error?: ConverterError;
}

export interface BatchProgress {
  completed: number;
  total: number;
  currentItemId: string | null;
  items: BatchItem[];
}

export interface BatchOptions {
  perFileBudgetBytes?: number;
  batchOutputBudgetBytes?: number;
  entitled?: boolean;
}

/** The largest single input a batch item may read, unless the options say otherwise. */
export const DEFAULT_BATCH_FILE_BUDGET_BYTES = 64 * 1024 * 1024;

export type BatchClient = Pick<ConverterClient, "ingestMany" | "runTool" | "reset" | "cancel"> & {
  terminate?: () => void;
};

function batchError(code: string, message: string): ConverterError {
  return { code, message };
}

/** Owns its client exclusively while running; never run UI jobs on that same client. */
export class BatchRunner {
  #items: BatchItem[] = [];
  #listeners = new Set<(progress: BatchProgress) => void>();
  #active: BatchItem | null = null;
  #busy = false;
  #nextId = 1;
  #retainedBytes = 0;
  #toolId = "";
  #params: Record<string, unknown> = {};
  readonly #perFileBudget: number;
  readonly #outputBudget: number;

  constructor(
    private readonly client: BatchClient = new ConverterClient(),
    private readonly options: BatchOptions = {},
  ) {
    this.#perFileBudget = options.perFileBudgetBytes ?? DEFAULT_BATCH_FILE_BUDGET_BYTES;
    this.#outputBudget = options.batchOutputBudgetBytes ?? 256 * 1024 * 1024;
    for (const value of [this.#perFileBudget, this.#outputBudget])
      if (!Number.isSafeInteger(value) || value <= 0)
        throw batchError("invalid_params", "Batch byte budgets must be positive safe integers.");
  }

  get items(): BatchItem[] {
    return this.#items.map((item) => ({ ...item }));
  }

  get progress(): BatchProgress {
    return {
      completed: this.#items.filter((item) => item.status !== "queued" && item.status !== "running")
        .length,
      total: this.#items.length,
      currentItemId: this.#active?.id ?? null,
      items: this.items,
    };
  }

  onProgress(listener: (progress: BatchProgress) => void): () => void {
    this.#listeners.add(listener);
    return () => {
      this.#listeners.delete(listener);
    };
  }

  async run(
    toolId: string,
    params: Record<string, unknown>,
    files: File[],
  ): Promise<BatchProgress> {
    this.#assertIdle();
    const tool = toolCatalog().find((candidate) => candidate.id === toolId);
    if (!tool) throw batchError("unknown_tool", `Unknown tool: ${toolId}`);
    if (tool.inputs.arity !== "one")
      throw batchError("wrong_input", "Batch execution requires a single-input tool.");
    this.#toolId = toolId;
    this.#params = structuredClone(params);
    this.#retainedBytes = 0;
    this.#items = files.map((file) => ({
      id: `batch-item-${this.#nextId++}`,
      file,
      status: "queued",
    }));
    return this.#execute();
  }

  /** Stops the batch and releases the worker this runner owns. */
  dispose(): void {
    this.cancelBatch();
    this.client.terminate?.();
  }

  cancelBatch(): void {
    for (const item of this.#items) if (item.status === "queued") item.status = "cancelled";
    this.#cancelActive();
    this.#emit();
  }

  cancelItem(itemId: string): void {
    const item = this.#items.find((candidate) => candidate.id === itemId);
    if (item?.status === "queued") item.status = "cancelled";
    if (item === this.#active) this.#cancelActive();
    this.#emit();
  }

  async retryFailed(): Promise<BatchProgress> {
    this.#assertIdle();
    for (const item of this.#items)
      if (item.status === "failed" || item.status === "cancelled") {
        item.status = "queued";
        delete item.error;
      }
    return this.#execute();
  }

  /** A File can be downloaded with downloadOutput({ kind: "memory", blob: file }, file.name). */
  async packageBatchZip(archiveName = "batch.zip"): Promise<File> {
    this.#assertIdle();
    const entries: ZipEntryInput[] = [];
    for (const item of this.#items) {
      if (item.status !== "done" || !item.result) continue;
      for (const artifact of item.result.artifacts) {
        const blob = await openOutput(artifact.location);
        entries.push({ name: artifact.name, bytes: new Uint8Array(await blob.arrayBuffer()) });
      }
    }
    const bytes = createZipArchive(entries, { maxTotalBytes: this.#outputBudget });
    const name = archiveName.replaceAll("\\", "/").split("/").pop() || "batch.zip";
    return new File([bytes], /\.zip$/i.test(name) ? name : `${name}.zip`, {
      type: "application/zip",
    });
  }

  #assertIdle(): void {
    if (this.#busy) throw batchError("batch_running", "Wait for the current batch to finish.");
  }

  #cancelActive(): void {
    if (this.#active?.status !== "running") return;
    this.#active.status = "cancelled";
    this.client.cancel();
  }

  #emit(): void {
    for (const listener of this.#listeners) {
      try {
        listener(this.progress);
      } catch {
        /* A view callback must not interrupt worker cleanup. */
      }
    }
  }

  async #execute(): Promise<BatchProgress> {
    this.#busy = true;
    try {
      this.#emit();
      for (const item of this.#items) {
        if (item.status !== "queued") continue;
        await this.#runItem(item);
      }
      return this.progress;
    } finally {
      this.#busy = false;
    }
  }

  async #runItem(item: BatchItem): Promise<void> {
    this.#active = item;
    item.status = "running";
    this.#emit();
    let retained: ToolExecutionResult | undefined;
    try {
      if (this.#isCancelled(item)) return;
      if (item.file.size > this.#perFileBudget)
        throw batchError("limit_exceeded", "The input exceeds the per-file batch byte budget.");
      if (this.#retainedBytes >= this.#outputBudget)
        throw batchError("limit_exceeded", "The batch output byte budget is exhausted.");
      const inputs = await this.client.ingestMany([item.file]);
      if (this.#isCancelled(item)) return;
      const result = await this.client.runTool(this.#toolId, structuredClone(this.#params), {
        inputIds: inputs.map((input) => input.id),
        budgetBytes: this.#perFileBudget,
        entitled: this.options.entitled,
      });
      if (this.#isCancelled(item)) return;
      retained = await this.#retain(result, item);
    } catch (error) {
      if (!this.#isCancelled(item)) {
        item.status = "failed";
        item.error = isConverterError(error)
          ? error
          : batchError("batch_failed", "The batch item could not finish.");
      }
    } finally {
      try {
        // Reset clears both input and output slots, after outputs have been detached.
        await this.client.reset();
      } catch (error) {
        this.client.cancel();
        if (!this.#isCancelled(item)) {
          item.status = "failed";
          item.error = isConverterError(error)
            ? error
            : batchError("cleanup_failed", "The worker input could not be cleared.");
        }
      }
      if (item.status === "running" && retained) {
        item.result = retained;
        item.status = "done";
        this.#retainedBytes += retained.artifacts.reduce(
          (total, artifact) => total + artifact.size,
          0,
        );
      }
      this.#active = null;
      this.#emit();
    }
  }

  #isCancelled(item: BatchItem): boolean {
    return item.status === "cancelled";
  }

  async #retain(result: ToolExecutionResult, item: BatchItem): Promise<ToolExecutionResult> {
    let total = this.#retainedBytes;
    const artifacts: ToolExecutionResult["artifacts"] = [];
    for (const artifact of result.artifacts) {
      if (
        !Number.isSafeInteger(artifact.size) ||
        artifact.size < 0 ||
        total + artifact.size > this.#outputBudget
      )
        throw batchError("limit_exceeded", "The results exceed the batch output byte budget.");
      const stored = await openOutput(artifact.location);
      total += stored.size;
      if (total > this.#outputBudget)
        throw batchError("limit_exceeded", "The results exceed the batch output byte budget.");
      // OPFS File snapshots can become unreadable when reset deletes their backing files.
      const blob =
        artifact.location.kind === "memory"
          ? stored
          : new Blob([await stored.arrayBuffer()], { type: artifact.mediaType });
      if (this.#isCancelled(item)) throw batchError("cancelled", "The batch item was cancelled.");
      artifacts.push({ ...artifact, size: blob.size, location: { kind: "memory", blob } });
    }
    const primary = artifacts[0];
    if (!primary) throw batchError("no_output", "The tool returned no batch artifacts.");
    return { ...result, artifacts, primaryLocation: primary.location };
  }
}
