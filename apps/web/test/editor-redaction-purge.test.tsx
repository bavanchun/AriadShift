// @vitest-environment jsdom

import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import type { WorkspaceRun } from "@/components/workspace/types";
import { useEditorWorkspace } from "@/components/workspace/use-editor-workspace";
import type { ToolExecutionResult } from "@/converter/protocol";
import { fakeClient, tool, workspaceFile } from "./workspace-fixtures";

const purge = vi.fn(async (_docId: string, _reason: string) => ({ purgedCount: 2 }));
const dispose = vi.fn(async () => undefined);

vi.mock("@/editor/storage", async (original) => ({
  ...(await original<typeof import("@/editor/storage")>()),
  createEditorVersionStore: () => ({ purgeDocumentHistory: purge, dispose }),
}));

afterEach(() => {
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

  it("releases the store when the workspace goes away", async () => {
    const hook = mount(finished("pdf.redact", "key-a.pdf"));
    await waitFor(() => expect(purge).toHaveBeenCalled());
    hook.unmount();
    expect(dispose).toHaveBeenCalledTimes(1);
  });
});
