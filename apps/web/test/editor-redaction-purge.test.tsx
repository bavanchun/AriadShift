// @vitest-environment jsdom

import {
  act,
  cleanup,
  fireEvent,
  render,
  renderHook,
  screen,
  waitFor,
} from "@testing-library/react";
import { DocumentWorkspace } from "@/components/workspace/document-workspace";
import type { WorkspaceRun } from "@/components/workspace/types";
import { useEditorWorkspace } from "@/components/workspace/use-editor-workspace";
import type { ToolExecutionResult } from "@/converter/protocol";
import { fakeBackend } from "./viewer-fixtures";
import { fakeClient, pdfFile, runResult, tool, workspaceFile } from "./workspace-fixtures";

const purge = vi.fn(async (_docId: string, _reason: string) => ({ purgedCount: 2 }));
const dispose = vi.fn(async () => undefined);

vi.mock("@/editor/storage", async (original) => ({
  ...(await original<typeof import("@/editor/storage")>()),
  createEditorVersionStore: () => ({ purgeDocumentHistory: purge, dispose }),
}));

// The browser cannot run `pdf.redact`, so the workspace is handed a finished redaction run, the
// way the desktop-backed runtime reports one. The run keeps its identity between renders.
let redactionSeeded = false;
const seededRuns = new Map<string, WorkspaceRun>();
vi.mock("@/components/workspace/use-workspace", async (original) => {
  const actual = await original<typeof import("@/components/workspace/use-workspace")>();
  return {
    ...actual,
    useWorkspace: (options: Parameters<typeof actual.useWorkspace>[0]) => {
      const workspace = actual.useWorkspace(options);
      if (!redactionSeeded || !workspace.active) return workspace;
      const key = workspace.active.key;
      let run = seededRuns.get(key);
      if (!run) {
        run = { ...finished("pdf.redact", key), result: runResult({ tool: "pdf.redact" }) };
        seededRuns.set(key, run);
      }
      return { ...workspace, run };
    },
  };
});

afterEach(() => {
  redactionSeeded = false;
  seededRuns.clear();
  cleanup();
  purge.mockClear();
  dispose.mockClear();
});

const result = {} as ToolExecutionResult;
const finished = (id: string, sourceKey?: string): WorkspaceRun => ({
  tool: tool(id),
  params: {},
  result,
  input: { size: 1, pageCount: 1, names: ["a.pdf"] },
  sourceKey,
});

function mount(initialRun: WorkspaceRun | null, active = workspaceFile("a.pdf")) {
  return renderHook(
    ({ run }: { run: WorkspaceRun | null }) =>
      useEditorWorkspace({ client: fakeClient(), active, mode: "view", run }),
    { initialProps: { run: initialRun } },
  );
}

describe("redaction purges the editor's saved history", () => {
  it("purges the redacted document's history even when Document mode is not open", async () => {
    const hook = mount(null);
    expect(purge).not.toHaveBeenCalled();
    hook.rerender({ run: finished("pdf.redact", "key-a.pdf") });
    await waitFor(() => expect(purge).toHaveBeenCalledWith("key-a.pdf", "redaction"));
    expect(hook.result.current.redacted.has("key-a.pdf")).toBe(true);
    expect(hook.result.current.purgeError).toBeNull();
  });

  it("leaves history alone for any other tool", async () => {
    const hook = mount(null);
    hook.rerender({ run: finished("pdf.rotate", "key-a.pdf") });
    await act(async () => undefined);
    expect(purge).not.toHaveBeenCalled();
    expect(hook.result.current.redacted.size).toBe(0);
  });

  it("reports a purge that failed instead of hiding it", async () => {
    purge.mockRejectedValueOnce(new Error("OPFS locked"));
    const hook = mount(finished("pdf.redact", "key-a.pdf"));
    await waitFor(() => expect(hook.result.current.purgeError).toBe("OPFS locked"));
  });

  it("purges again when the same document is redacted a second time", async () => {
    const hook = mount(finished("pdf.redact", "key-a.pdf"));
    await waitFor(() => expect(purge).toHaveBeenCalledTimes(1));
    hook.rerender({ run: finished("pdf.redact", "key-a.pdf") });
    await waitFor(() => expect(purge).toHaveBeenCalledTimes(2));
  });

  it("purges when a redaction was an earlier step of the chain", async () => {
    const hook = mount(null);
    hook.rerender({ run: { ...finished("pdf.rotate", "key-a.pdf"), redacts: true } });
    await waitFor(() => expect(purge).toHaveBeenCalledWith("key-a.pdf", "redaction"));
  });

  it("releases the store when the workspace goes away", async () => {
    const hook = mount(finished("pdf.redact", "key-a.pdf"));
    await waitFor(() => expect(purge).toHaveBeenCalled());
    hook.unmount();
    expect(dispose).toHaveBeenCalledTimes(1);
  });
});

describe("<DocumentWorkspace /> purge failure", () => {
  it("shows the alert while the document is still in View mode", async () => {
    purge.mockRejectedValueOnce(new Error("OPFS locked"));
    redactionSeeded = true;
    const view = render(
      <DocumentWorkspace client={fakeClient()} loadBackend={fakeBackend().load} />,
    );
    const input = view.container.querySelector("input[type=file]") as HTMLInputElement;
    await act(async () => {
      fireEvent.change(input, { target: { files: [pdfFile("a.pdf")] } });
    });
    const alert = await screen.findByText(/could not be removed\. OPFS locked/);
    expect(alert.getAttribute("role")).toBe("alert");
    expect(screen.getByTestId("workspace-mode-view").getAttribute("aria-pressed")).toBe("true");
  });
});
