// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { ToolResultCard } from "@/components/workspace/result-card";
import type { WorkspaceRun } from "@/components/workspace/types";
import { makePdf } from "../e2e/pdf-fixtures";
import { runResult, tool } from "./workspace-fixtures";

afterEach(cleanup);

function run(overrides: Partial<WorkspaceRun["result"]> = {}): WorkspaceRun {
  return {
    tool: tool("pdf.compress"),
    params: {},
    result: runResult(overrides),
    input: { size: 2048, pageCount: 8, names: ["report.pdf"] },
  };
}

const handlers = () => ({ onContinue: vi.fn(), onUndo: vi.fn(), onDownload: vi.fn() });

describe("<ToolResultCard />", () => {
  it("shows measured before and after numbers", () => {
    render(
      <ToolResultCard run={run({ metrics: { page_count: 8 }, elapsedMs: 1530 })} {...handlers()} />,
    );
    const table = screen.getByRole("table");
    expect(within(table).getByText("2.0 KB")).toBeTruthy();
    expect(within(table).getByText(/512 bytes/)).toBeTruthy();
    expect(screen.getByText("(−75%)")).toBeTruthy();
    expect(within(table).getAllByText("8")).toHaveLength(2);
    expect(within(table).getByText("1.5 s")).toBeTruthy();
  });

  it("shows an unknown page count as a dash and a size increase with a plus", () => {
    const grown = run();
    grown.input.size = 256;
    render(<ToolResultCard run={grown} {...handlers()} />);
    expect(screen.getByText("(+100%)")).toBeTruthy();
    expect(within(screen.getByRole("table")).getAllByText("—").length).toBeGreaterThan(0);
  });

  it("reads the page count from the output PDF when the tool reports none", async () => {
    const blob = new Blob([new Uint8Array(makePdf(5))], { type: "application/pdf" });
    const measured = run({ primaryLocation: { kind: "memory", blob } });
    render(<ToolResultCard run={measured} {...handlers()} />);
    const pages = screen.getByRole("row", { name: /Pages/ });
    await waitFor(() => expect(within(pages).getByText("5")).toBeTruthy());
  });

  it("lists warnings and the tool's JSON details", () => {
    render(
      <ToolResultCard
        run={run({
          warnings: [{ code: "w1", message: "Fonts were not embedded" } as never],
          data: { saved_bytes: 1536 },
        })}
        {...handlers()}
      />,
    );
    expect(screen.getByText("Fonts were not embedded")).toBeTruthy();
    expect(screen.getByText(/"saved_bytes": 1536/)).toBeTruthy();
  });

  it("downloads each output", () => {
    const actions = handlers();
    const value = run();
    render(<ToolResultCard run={value} {...actions} />);
    fireEvent.click(screen.getByRole("button", { name: "Download report.pdf" }));
    expect(actions.onDownload).toHaveBeenCalledWith(value.result.artifacts[0]);
  });

  it("continues with another tool using the output as the next input", () => {
    const actions = handlers();
    const value = run();
    render(<ToolResultCard run={value} {...actions} />);
    fireEvent.click(screen.getByRole("button", { name: "Continue with another tool" }));
    expect(actions.onContinue).toHaveBeenCalledWith(value.result.artifacts[0]);
  });

  it("undoes", () => {
    const actions = handlers();
    render(<ToolResultCard run={run()} {...actions} />);
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(actions.onUndo).toHaveBeenCalled();
  });

  it("offers the editor and recipes only when the host can provide them", () => {
    const { rerender } = render(<ToolResultCard run={run()} {...handlers()} />);
    expect(screen.queryByRole("button", { name: "Open in editor" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Save as recipe" })).toBeNull();

    const onOpenInEditor = vi.fn();
    const onSaveRecipe = vi.fn();
    const value = run();
    rerender(
      <ToolResultCard
        run={value}
        {...handlers()}
        onOpenInEditor={onOpenInEditor}
        onSaveRecipe={onSaveRecipe}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Open in editor" }));
    expect(onOpenInEditor).toHaveBeenCalledWith(value.result.artifacts[0]);
    fireEvent.click(screen.getByRole("button", { name: "Save as recipe" }));
    expect(onSaveRecipe).toHaveBeenCalledTimes(1);
  });

  it("reports an action that fails instead of leaving the user stuck", async () => {
    render(
      <ToolResultCard
        run={run()}
        {...handlers()}
        onDownload={vi.fn().mockRejectedValue(new Error("quota"))}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Download report.pdf" }));
    expect((await screen.findByRole("alert")).textContent).toContain("could not");
  });

  it("offers every output of a multi-file result", () => {
    const value = run();
    const second = {
      ...(value.result.artifacts[0] as NonNullable<(typeof value.result.artifacts)[0]>),
      name: "part-2.pdf",
    };
    value.result.artifacts.push(second);
    render(<ToolResultCard run={value} {...handlers()} />);
    expect(screen.getByRole("button", { name: "Download part-2.pdf" })).toBeTruthy();
    expect(screen.getAllByRole("button", { name: "Continue with another tool" })).toHaveLength(2);
  });
});
