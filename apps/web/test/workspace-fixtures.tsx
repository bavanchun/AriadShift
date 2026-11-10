import { ToastProvider, ToastViewport } from "@ariadshift/ui/components/toast";
import { render } from "@testing-library/react";
import type { ReactElement } from "react";
import type { Mock } from "vitest";
import type { WorkspaceClient, WorkspaceFile } from "@/components/workspace/types";
import type { PdfFileInsights } from "@/converter/insights";
import type { ToolExecutionResult } from "@/converter/protocol";
import { type ToolDescriptor, toolCatalog } from "@/converter/tool-catalog";

const catalog = toolCatalog();

export function tool(id: string): ToolDescriptor {
  const found = catalog.find((entry) => entry.id === id);
  if (!found) throw new Error(`Missing tool ${id}`);
  return found;
}

export function renderWithToasts(ui: ReactElement) {
  return render(
    <ToastProvider>
      {ui}
      <ToastViewport />
    </ToastProvider>,
  );
}

export function insights(overrides: Partial<PdfFileInsights> = {}): PdfFileInsights {
  return {
    pageCount: 4,
    scannedPageCount: 0,
    scannedPages: [],
    blankPageCount: 0,
    blankPages: [],
    estimatedImageDpi: { min: null, max: null, lowDpiPages: [] },
    emptyRequiredFormFields: { count: 0, names: [] },
    signatureFieldCount: 0,
    hasMetadata: false,
    metadataKeys: [],
    hasEditHistory: false,
    encrypted: false,
    linearized: false,
    hasForms: false,
    hasAnnotations: false,
    hasAttachments: false,
    hasJavascript: false,
    outlineCount: 0,
    suggestedTools: [],
    warnings: [],
    recycle: false,
    ...overrides,
  };
}

export function pdfFile(name: string, size = 2048): File {
  const file = new File([new Uint8Array(8)], name, { type: "application/pdf" });
  Object.defineProperty(file, "size", { value: size });
  return file;
}

export function workspaceFile(name: string, overrides: Partial<WorkspaceFile> = {}): WorkspaceFile {
  return {
    id: `slot-${name}`,
    key: `key-${name}`,
    name,
    size: 2048,
    pageCount: 4,
    file: pdfFile(name),
    ...overrides,
  };
}

export function runResult(overrides: Partial<ToolExecutionResult> = {}): ToolExecutionResult {
  const blob = new Blob([new Uint8Array(512)], { type: "application/pdf" });
  return {
    tool: "pdf.rotate",
    format: "pdf",
    mediaType: "application/pdf",
    extension: "pdf",
    primaryLocation: { kind: "memory", blob },
    artifacts: [
      {
        name: "report.pdf",
        size: 512,
        format: "pdf",
        mediaType: "application/pdf",
        extension: "pdf",
        location: { kind: "memory", blob },
      },
    ],
    data: null,
    warnings: [],
    metrics: {},
    elapsedMs: 42,
    recycle: false,
    ...overrides,
  };
}

export type FakeClient = { [K in keyof WorkspaceClient]: Mock<WorkspaceClient[K]> };

/** A converter stub: ingestion hands out sequential slot ids, every file is a PDF. */
export function fakeClient(overrides: Partial<WorkspaceClient> = {}): FakeClient {
  let slots = 0;
  const client = {
    ingestMany: vi.fn(async (files: File[]) =>
      files.map((file) => ({
        id: `slot-${++slots}`,
        name: file.name,
        size: file.size,
        inspection: { format: "pdf" } as never,
        recycle: false,
      })),
    ),
    listTools: vi.fn(async () => catalog),
    runTool: vi.fn(async () => runResult()),
    inspectInsights: vi.fn(async () => insights()),
    cancel: vi.fn(),
    ...overrides,
  };
  return client as unknown as FakeClient;
}
