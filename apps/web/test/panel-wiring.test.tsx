// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { ToolPanel } from "@/components/tools/panels";
import { COMPARE_TOOLS } from "@/components/tools/panels/compare";
import { COMPRESS_TOOLS } from "@/components/tools/panels/compress";
import { IMPOSITION_TOOLS } from "@/components/tools/panels/imposition";
import { OCR_TOOLS } from "@/components/tools/panels/ocr";
import { ORGANISER_TOOLS } from "@/components/tools/panels/organiser";
import { REDACT_TOOLS } from "@/components/tools/panels/redact";
import { validateRegex } from "@/components/tools/panels/redact/targets";
import { SECURITY_TOOLS } from "@/components/tools/panels/security";
import { SIGN_TOOLS } from "@/components/tools/panels/sign";
import { WATERMARK_TOOLS } from "@/components/tools/panels/watermark";
import { defaultSettings, overlayItems } from "@/components/tools/panels/watermark/watermark-model";
import { BatchSection } from "@/components/workspace/batch-section";
import { ToolReportCards } from "@/components/workspace/report-cards";
import { ToolInspector } from "@/components/workspace/tool-inspector";
import { BatchRunner } from "@/converter/batch";
import { toolCatalog } from "@/converter/tool-catalog";
import { renderWithToasts, runResult, tool, workspaceFile } from "./workspace-fixtures";

afterEach(cleanup);

const SETS: Array<[string, readonly string[]]> = [
  ["organiser-panel", ORGANISER_TOOLS],
  ["imposition-panel", IMPOSITION_TOOLS],
  ["compress-panel", COMPRESS_TOOLS],
  ["security-panel", SECURITY_TOOLS],
  ["watermark-panel", WATERMARK_TOOLS],
  ["ocr-panel", OCR_TOOLS],
  ["redact-panel", REDACT_TOOLS],
  ["sign-panel", SIGN_TOOLS],
  ["compare-panel", COMPARE_TOOLS],
];
// A studio may hold several panels (watermark and its removal studio), so match by prefix.
const PANEL_SLOTS = SETS.map(([slot]) => slot.replace(/-panel$/, ""));
const slotOf = (name: string) => `[data-slot^="${name}"]`;
const files = [workspaceFile("a.pdf"), workspaceFile("b.pdf")];

describe("<ToolPanel />", () => {
  it.each(SETS)("opens the %s studio for each of its tools", (slot, ids) => {
    const registry = new Set(toolCatalog().map((entry) => entry.id));
    for (const id of ids.filter((entry) => registry.has(entry))) {
      const { container, unmount } = render(
        <ToolPanel tool={tool(id)} files={files} pageCount={4} onChange={vi.fn()} />,
      );
      const own = slot.replace(/-panel$/, "");
      expect(container.querySelector(slotOf(own)), id).toBeTruthy();
      for (const other of PANEL_SLOTS.filter((entry) => entry !== own))
        expect(container.querySelector(slotOf(other)), `${id} vs ${other}`).toBeNull();
      unmount();
    }
  });

  it("keeps the generic form for a tool without a studio", () => {
    const covered = new Set(SETS.flatMap(([, ids]) => ids));
    const plain = toolCatalog().find((entry) => !covered.has(entry.id));
    expect(plain).toBeTruthy();
    if (!plain) return;
    const onChange = vi.fn();
    const { container } = render(
      <ToolPanel tool={plain} files={files} pageCount={4} onChange={onChange} />,
    );
    for (const slot of PANEL_SLOTS) expect(container.querySelector(slotOf(slot))).toBeNull();
    expect(onChange).toHaveBeenCalled();
  });
});

describe("<ToolInspector /> studio wiring", () => {
  it("runs the compare tool with the inputs in the order the A ⇄ B swap chose", () => {
    const onRun = vi.fn();
    renderWithToasts(
      <ToolInspector
        tool={tool("pdf.compare")}
        files={files}
        pageCount={4}
        onRun={onRun}
        onCancel={vi.fn()}
      />,
    );
    const run = screen.getByRole("button", { name: "Run tool on this device" });
    fireEvent.click(run);
    expect(onRun).toHaveBeenLastCalledWith(
      { mode: "text" },
      { inputIds: ["slot-a.pdf", "slot-b.pdf"] },
    );
    fireEvent.click(screen.getByRole("button", { name: "Swap baseline and revised" }));
    fireEvent.click(run);
    expect(onRun).toHaveBeenLastCalledWith(
      { mode: "text" },
      { inputIds: ["slot-b.pdf", "slot-a.pdf"] },
    );
  });

  it("leaves the input choice to the workspace for other tools", () => {
    const onRun = vi.fn();
    renderWithToasts(
      <ToolInspector
        tool={tool("pdf.rotate")}
        files={files}
        pageCount={4}
        onRun={onRun}
        onCancel={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Run tool on this device" }));
    expect(onRun).toHaveBeenLastCalledWith({ angle: 90, pages: "all" });
  });

  it("mirrors the parameters to the host", () => {
    const onParamsChange = vi.fn();
    renderWithToasts(
      <ToolInspector
        tool={tool("pdf.rotate")}
        files={files}
        pageCount={4}
        onRun={vi.fn()}
        onParamsChange={onParamsChange}
        onCancel={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "180°" }));
    expect(onParamsChange).toHaveBeenLastCalledWith(
      { angle: 180, pages: "all" },
      expect.objectContaining({ isValid: true }),
    );
  });
});

describe("<ToolReportCards />", () => {
  const checks = {
    pdfium_text: "pass",
    core_text: "pass",
    region_render: "pass",
    image_pixels: "pass",
    object_graph: "pass",
    rewrite_history: "pass",
  };

  it("shows the redaction verification a finished pdf.redact returned", () => {
    render(
      <ToolReportCards
        toolId="pdf.redact"
        result={runResult({
          data: { report: { status: "verified", checks, target_count: 2, match_count: 5 } },
        })}
      />,
    );
    expect(
      document
        .querySelector('[data-slot="redaction-verification-card"]')
        ?.getAttribute("data-status"),
    ).toBe("verified");
  });

  it("reads a report the tool published as a JSON artifact", async () => {
    const json = new Blob(
      [JSON.stringify({ status: "unverifiable", checks, target_count: 0, match_count: 0 })],
      {
        type: "application/json",
      },
    );
    const base = runResult();
    const [first] = base.artifacts;
    if (!first) throw new Error("fixture has no artifact");
    render(
      <ToolReportCards
        toolId="pdf.redact_verify"
        result={runResult({
          artifacts: [
            {
              ...first,
              name: "doc.verify.json",
              mediaType: "application/json",
              location: { kind: "memory", blob: json },
            },
          ],
        })}
      />,
    );
    await waitFor(() =>
      expect(
        document
          .querySelector('[data-slot="redaction-verification-card"]')
          ?.getAttribute("data-status"),
      ).toBe("unverifiable"),
    );
  });

  it("shows the signature validation of pdf.sign_verify", () => {
    render(
      <ToolReportCards
        toolId="pdf.sign_verify"
        result={runResult({
          data: { valid: false, integrity: false, signature_count: 0, signatures: [] },
        })}
      />,
    );
    expect(screen.getByText("This document has no signatures.")).toBeTruthy();
  });

  it("renders nothing for other tools or an unrecognised report", () => {
    const { container, rerender } = render(
      <ToolReportCards
        toolId="pdf.rotate"
        result={runResult({ data: { status: "verified", checks } })}
      />,
    );
    expect(container.firstChild).toBeNull();
    rerender(
      <ToolReportCards toolId="pdf.redact" result={runResult({ data: { nothing: true } })} />,
    );
    expect(container.firstChild).toBeNull();
  });
});

describe("<BatchSection />", () => {
  function stubRunner() {
    let slots = 0;
    const client = {
      ingestMany: vi.fn(async (list: File[]) =>
        list.map((file) => ({
          id: `slot-${++slots}`,
          name: file.name,
          size: file.size,
          inspection: null,
          recycle: false,
        })),
      ),
      runTool: vi.fn(async (_toolId: string, _params: Record<string, unknown>) => runResult()),
      reset: vi.fn(async () => undefined),
      cancel: vi.fn(),
      terminate: vi.fn(),
    };
    return { client, runner: new BatchRunner(client as never) };
  }

  it("queues every file, runs the tool on each and packages the results as a zip", async () => {
    const { client, runner } = stubRunner();
    const created: Blob[] = [];
    const createObjectURL = vi.spyOn(URL, "createObjectURL").mockImplementation((blob) => {
      created.push(blob as Blob);
      return "blob:test";
    });
    vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => undefined);
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => undefined);
    renderWithToasts(
      <BatchSection
        tool={tool("pdf.rotate")}
        params={{ angle: 90, pages: "all" }}
        paramsValid
        files={files}
        createRunner={() => runner}
        onRemoveFile={vi.fn()}
      />,
    );
    expect(screen.getAllByText("Queued")).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "Run batch" }));
    expect(await screen.findByText("2 of 2 done")).toBeTruthy();
    expect(client.runTool).toHaveBeenCalledTimes(2);
    expect(client.runTool.mock.calls[0]?.[0]).toBe("pdf.rotate");
    expect(client.runTool.mock.calls[0]?.[1]).toEqual({ angle: 90, pages: "all" });
    fireEvent.click(screen.getByRole("button", { name: "Download all (.zip)" }));
    await waitFor(() => expect(createObjectURL).toHaveBeenCalled());
    expect(created.at(-1)?.type).toBe("application/zip");
  });

  it("will not run with invalid parameters and drops a queued file from the workspace", () => {
    const { client, runner } = stubRunner();
    const onRemoveFile = vi.fn();
    renderWithToasts(
      <BatchSection
        tool={tool("pdf.rotate")}
        params={{}}
        paramsValid={false}
        files={files}
        createRunner={() => runner}
        onRemoveFile={onRemoveFile}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Run batch" }));
    expect(screen.getByRole("alert").textContent).toContain("Fix the highlighted options");
    expect(client.runTool).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Cancel a.pdf" }));
    expect(onRemoveFile).toHaveBeenCalledWith("slot-a.pdf");
  });

  it("retries only the files that failed", async () => {
    const { client, runner } = stubRunner();
    client.runTool.mockRejectedValueOnce({ code: "failed", message: "Broken page tree" });
    renderWithToasts(
      <BatchSection
        tool={tool("pdf.rotate")}
        params={{ angle: 90, pages: "all" }}
        paramsValid
        files={files}
        createRunner={() => runner}
        onRemoveFile={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Run batch" }));
    expect(await screen.findByText("Broken page tree")).toBeTruthy();
    expect(screen.getByText("1 of 2 done")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Retry failed" }));
    expect(await screen.findByText("2 of 2 done")).toBeTruthy();
    expect(client.runTool).toHaveBeenCalledTimes(3);
  });

  it("releases the worker when it unmounts", () => {
    const { client, runner } = stubRunner();
    const view = renderWithToasts(
      <BatchSection
        tool={tool("pdf.rotate")}
        params={{}}
        paramsValid
        files={files}
        createRunner={() => runner}
        onRemoveFile={vi.fn()}
      />,
    );
    view.unmount();
    expect(client.terminate).toHaveBeenCalled();
  });
});

describe("review follow-ups", () => {
  it("accepts inline regex flags the engine understands", () => {
    expect(validateRegex("(?i)invoice")).toBeNull();
    expect(validateRegex("(?i)(?s)a.b")).toBeNull();
    expect(validateRegex("x(?i:abc)y")).toBeNull();
    expect(validateRegex("(?i)(")).toMatch(/^Invalid regular expression/);
    expect(validateRegex("(?=a)b")).toBe("Lookahead and lookbehind are not supported.");
  });

  it("previews page numbers without expanding {date}, which that tool does not accept", () => {
    const settings = {
      ...defaultSettings("pdf.page_numbers"),
      format: "Page {n} of {total} {date}",
    };
    const text = overlayItems("pdf.page_numbers", settings, 4)[0]?.text;
    expect(text).toBe("Page 1 of 4 {date}");
  });
});
