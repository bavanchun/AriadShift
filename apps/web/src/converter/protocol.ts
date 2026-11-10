// Messages between the converter page and its Web Worker.
import type {
  FormatId,
  InspectResult,
  Notice,
  Plan,
  PlanStep,
  ProfileId,
} from "@ariadshift/ariad-wasm";

import type { PdfFileInsights } from "./insights";
import type { ToolDescriptor } from "./tool-catalog";

export type StorageKind = "opfs" | "memory";

export type WorkerRequest =
  | { type: "init" }
  | { type: "ingest"; file: File }
  | { type: "ingestMany"; files: File[] }
  | { type: "listTools" }
  | {
      type: "runTool";
      toolId: string;
      inputIds?: string[];
      params: Record<string, unknown>;
      budgetBytes?: number;
      entitled?: boolean;
    }
  | { type: "inspectInsights"; inputId?: string }
  | { type: "plan"; to: FormatId; profile: ProfileId }
  | { type: "convert"; to: FormatId; profile: ProfileId }
  | { type: "reset" }
  | { type: "retire"; sessions: string[] };

export interface InitResult {
  version: string;
  /** This worker's storage session; its OPFS folder is `ariadshift/<session>`. */
  session: string;
  storage: StorageKind;
  /** Largest input accepted when files are kept in memory. */
  memoryLimitBytes: number;
  /** Largest input the browser converter accepts. */
  inputLimitBytes: number;
  /** URLs the worker loaded, for the offline cache. */
  resources: string[];
}

export interface IngestResult {
  name: string;
  size: number;
  inspection: InspectResult;
  /** The module grew large; replace the worker before the next file. */
  recycle: boolean;
}

/** Auxiliary files may be valid tool inputs without being convertible documents. */
export interface InputIngestResult extends Omit<IngestResult, "inspection"> {
  id: string;
  inspection: InspectResult | null;
  inspectionError?: ConverterError;
}

export type OutputLocation =
  | { kind: "opfs"; directory: string[]; name: string }
  | { kind: "memory"; blob: Blob };

export interface ConvertedFile {
  /** Assets loaded during this route, including lazy engine chunks. */
  resources?: string[];
  name: string;
  size: number;
  format: FormatId;
  mediaType: string;
  route: PlanStep[];
  warnings: Notice[];
  elapsedMs: number;
  location: OutputLocation;
  /** The module grew large; replace the worker before the next file. */
  recycle: boolean;
}

export interface ToolExecutionArtifact {
  name: string;
  size: number;
  format: string;
  mediaType: string;
  extension: string;
  location: OutputLocation;
}

export interface ToolExecutionResult {
  tool: string;
  format: string;
  mediaType: string;
  extension: string;
  primaryLocation: OutputLocation;
  artifacts: ToolExecutionArtifact[];
  data: Record<string, unknown> | null;
  warnings: Notice[];
  metrics: Record<string, unknown>;
  elapsedMs: number;
  recycle: boolean;
}

export interface ResultMap {
  ingestMany: InputIngestResult[];
  listTools: ToolDescriptor[];
  runTool: ToolExecutionResult;
  inspectInsights: PdfFileInsights;
  init: InitResult;
  ingest: IngestResult;
  plan: Plan;
  convert: ConvertedFile;
  reset: null;
  /** Folders of replaced workers that are still held and left for a later sweep. */
  retire: { held: number };
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
