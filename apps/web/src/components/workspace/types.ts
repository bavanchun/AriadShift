// Shared shapes of the document workspace.
import type { PdfFileInsights } from "@/converter/insights";
import type { InputIngestResult, ToolExecutionResult } from "@/converter/protocol";
import type { ToolDescriptor } from "@/converter/tool-catalog";

/** The slice of `ConverterClient` the workspace uses; tests supply a stub. */
export interface WorkspaceClient {
  ingestMany(files: File[]): Promise<InputIngestResult[]>;
  listTools(): Promise<ToolDescriptor[]>;
  runTool(
    toolId: string,
    params: Record<string, unknown>,
    options?: { inputIds?: string[]; budgetBytes?: number; entitled?: boolean },
  ): Promise<ToolExecutionResult>;
  inspectInsights(inputId?: string): Promise<PdfFileInsights>;
  cancel(): void;
}

export interface WorkspaceFile {
  /** Input slot id in the converter's file store; it changes whenever the set is re-ingested. */
  id: string;
  /** Stable for the life of the file, across re-ingestion; use it as a React key or viewer id. */
  key: string;
  name: string;
  size: number;
  /** Known once the file's insights have been read; null for non-PDF files. */
  pageCount: number | null;
  file: File;
  /** Why the file is not a usable document, when the converter could not read it. */
  problem?: string;
}

export type InsightsState =
  | { status: "loading" }
  | { status: "ready"; insights: PdfFileInsights }
  | { status: "failed"; message: string };

export type ThreadStatus = "pending" | "running" | "completed" | "skipped" | "failed";

export interface ThreadStep {
  id: string;
  label: string;
  status: ThreadStatus;
}

/** A step of the tool chain run in this workspace, kept so it can be saved as a recipe. */
export interface RecipeStep {
  toolId: string;
  params: Record<string, unknown>;
}

export interface WorkspaceRun {
  tool: ToolDescriptor;
  params: Record<string, unknown>;
  result: ToolExecutionResult;
  /** Bytes and pages of the input the tool received. */
  input: { size: number; pageCount: number | null; names: string[] };
}
