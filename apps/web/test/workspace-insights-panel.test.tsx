// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { FileInsightsPanel } from "@/components/workspace/insights-panel";
import { toolCatalog } from "@/converter/tool-catalog";
import { insights } from "./workspace-fixtures";

afterEach(cleanup);
const tools = toolCatalog();

describe("<FileInsightsPanel />", () => {
  it("shows a status while the document is read, and a reason when it cannot be", () => {
    const { rerender } = render(
      <FileInsightsPanel state={{ status: "loading" }} tools={tools} onOpenTool={vi.fn()} />,
    );
    expect(screen.getByRole("status").textContent).toContain("Reading the document on this device");
    rerender(
      <FileInsightsPanel
        state={{ status: "failed", message: "Bad xref" }}
        tools={tools}
        onOpenTool={vi.fn()}
      />,
    );
    expect(screen.getByRole("alert").textContent).toContain("Bad xref");
  });

  it("reports scanned and blank pages, DPI, form fields, signatures and flags", () => {
    render(
      <FileInsightsPanel
        tools={tools}
        onOpenTool={vi.fn()}
        state={{
          status: "ready",
          insights: insights({
            pageCount: 12,
            scannedPageCount: 3,
            scannedPages: [2, 3, 4],
            blankPageCount: 1,
            blankPages: [7],
            estimatedImageDpi: { min: 72, max: 300, lowDpiPages: [2, 3] },
            emptyRequiredFormFields: { count: 2, names: ["name", "date"] },
            signatureFieldCount: 1,
            hasMetadata: true,
            hasEditHistory: true,
            encrypted: true,
            linearized: true,
          }),
        }}
      />,
    );
    expect(screen.getByText("12 pages")).toBeTruthy();
    expect(screen.getByText("3 scanned pages")).toBeTruthy();
    expect(screen.getByText("1 blank page")).toBeTruthy();
    expect(screen.getByText("Images about 72–300 DPI")).toBeTruthy();
    expect(screen.getByText("Low resolution on 2 pages (2, 3)")).toBeTruthy();
    expect(screen.getByText("2 empty required fields")).toBeTruthy();
    expect(screen.getByText("1 signature")).toBeTruthy();
    expect(screen.getByText("Contains metadata")).toBeTruthy();
    expect(screen.getByText("Has edit history")).toBeTruthy();
    expect(screen.getByText("Encrypted")).toBeTruthy();
    expect(screen.getByText("Linearized")).toBeTruthy();
  });

  it("says so when the resolution cannot be estimated and nothing needs attention", () => {
    render(
      <FileInsightsPanel
        tools={tools}
        onOpenTool={vi.fn()}
        state={{ status: "ready", insights: insights() }}
      />,
    );
    expect(screen.getByText("Image resolution unknown")).toBeTruthy();
    expect(screen.getByText(/Nothing needs attention/)).toBeTruthy();
  });

  it("offers each suggested tool as one click that opens it", () => {
    const onOpenTool = vi.fn();
    render(
      <FileInsightsPanel
        tools={tools}
        onOpenTool={onOpenTool}
        state={{
          status: "ready",
          insights: insights({
            blankPageCount: 1,
            blankPages: [3],
            suggestedTools: ["pdf.delete_pages", "pdf.not_a_tool"],
          }),
        }}
      />,
    );
    // A suggestion the catalogue does not know is not offered as a dead button.
    expect(screen.getAllByRole("button")).toHaveLength(1);
    fireEvent.click(screen.getByRole("button", { name: "Open Delete pages" }));
    expect(onOpenTool).toHaveBeenCalledWith("pdf.delete_pages");
  });
});
