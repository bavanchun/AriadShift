// Messages between the converter page and its Web Worker.
import type {
  FormatId,
  InspectResult,
  Notice,
  Plan,
  PlanStep,
  ProfileId,
} from "@ariadshift/ariad-wasm";

export type StorageKind = "opfs" | "memory";

export type WorkerRequest =
  | { type: "init" }
  | { type: "ingest"; file: File }
  | { type: "plan"; to: FormatId; profile: ProfileId }
  | { type: "convert"; to: FormatId; profile: ProfileId }
  | { type: "reset" };

export interface InitResult {
  version: string;
  storage: StorageKind;
  /** Largest input accepted when files are kept in memory. */
  memoryLimitBytes: number;
  /** URLs the worker loaded, for the offline cache. */
  resources: string[];
}

export interface IngestResult {
  name: string;
  size: number;
  inspection: InspectResult;
}

export type OutputLocation =
  | { kind: "opfs"; directory: string[]; name: string }
  | { kind: "memory"; blob: Blob };

export interface ConvertedFile {
  name: string;
  size: number;
  format: FormatId;
  mediaType: string;
  route: PlanStep[];
  warnings: Notice[];
  elapsedMs: number;
  location: OutputLocation;
}

export interface ResultMap {
  init: InitResult;
  ingest: IngestResult;
  plan: Plan;
  convert: ConvertedFile;
  reset: null;
}

/** An error from the wasm module, the file store or the worker itself. */
export interface ConverterError {
  code: string;
  message: string;
  /** The worker must be replaced before the next request. */
  fatal?: boolean;
  [detail: string]: unknown;
}

export type RequestEnvelope = WorkerRequest & { id: number };

export type WorkerResponse =
  | { id: number; ok: true; result: ResultMap[keyof ResultMap] }
  | { id: number; ok: false; error: ConverterError };

export function isConverterError(value: unknown): value is ConverterError {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as { code?: unknown }).code === "string" &&
    typeof (value as { message?: unknown }).message === "string"
  );
}
