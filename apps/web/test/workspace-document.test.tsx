// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { DocumentWorkspace } from "@/components/workspace/document-workspace";
import { STORE_INPUT_LIMIT_BYTES } from "@/converter/file-store";
import { fakeBackend } from "./viewer-fixtures";
import { type FakeClient, fakeClient, insights, pdfFile, runResult } from "./workspace-fixtures";

afterEach(cleanup);

async function open(
  client = fakeClient(),
  extra: Partial<React.ComponentProps<typeof DocumentWorkspace>> = {},
) {
  const viewer = fakeBackend();
  const view = render(<DocumentWorkspace client={client} loadBackend={viewer.load} {...extra} />);
  const input = () => view.container.querySelector("input[type=file]") as HTMLInputElement;
  return { ...view, client, viewer, input };
}

async function drop(input: () => HTMLInputElement, ...files: File[]) {
  await act(async () => {
    fireEvent.change(input(), { target: { files } });
  });
}

/** File names handed to the nth `ingestMany` call. */
const ingested = (client: FakeClient, call: number): string[] =>
  ((client.ingestMany.mock.calls[call]?.[0] ?? []) as File[]).map((file) => file.name);

const resultCard = () =>
  waitFor(() => {
    const found = document.querySelector('[data-slot="tool-result"]') as HTMLElement | null;
    expect(found).toBeTruthy();
    return found as HTMLElement;
  });

const pickTool = (id: string) =>
  fireEvent.change(screen.getByLabelText("Choose a tool"), { target: { value: id } });

describe("<DocumentWorkspace />", () => {
  it("starts empty, says nothing is uploaded, and lists the catalogue's tools", async () => {
    await open();
    expect(screen.getByText("On this device · 0 bytes uploaded")).toBeTruthy();
    expect(screen.getByText("Add a PDF to preview and organise its pages.")).toBeTruthy();
    await waitFor(() => expect(screen.getByRole("option", { name: "Rotate" })).toBeTruthy());
  });

  it("reads a dropped PDF into insights with one-click next steps that open the tool", async () => {
    const client = fakeClient({
      inspectInsights: vi.fn(async () =>
        insights({
          pageCount: 6,
          blankPageCount: 1,
          blankPages: [3],
          suggestedTools: ["pdf.delete_pages"],
        }),
      ),
    });
    const { input } = await open(client);
    await drop(input, pdfFile("report.pdf", 4096));
    const panel = await waitFor(() => {
      const found = document.querySelector('[data-slot="file-insights"]') as HTMLElement;
      expect(within(found).getByText("6 pages")).toBeTruthy();
      return found;
    });
    expect(within(panel).getByText("1 blank page")).toBeTruthy();
    expect(client.ingestMany).toHaveBeenCalledTimes(1);
    expect(client.inspectInsights).toHaveBeenCalledWith("slot-1");
    // The tray learned the page count from the insights.
    expect(
      within(screen.getByRole("list", { name: "Files in this workspace" })).getByText(
        /4\.0 KB · 6 pages/,
      ),
    ).toBeTruthy();
    fireEvent.click(within(panel).getByRole("button", { name: "Open Delete pages" }));
    expect(document.querySelector('[data-slot="tool-inspector"]')?.textContent).toContain(
      "Delete pages",
    );
  });

  it("shows why a document could not be analysed", async () => {
    const client = fakeClient({
      inspectInsights: vi.fn().mockRejectedValue({ code: "bad_pdf", message: "Bad xref" }),
    });
    const { input } = await open(client);
    await drop(input, pdfFile("broken.pdf"));
    expect(await screen.findByText(/could not be analysed\. Bad xref/)).toBeTruthy();
  });

  it("runs a tool on this device within the byte budget, then continues with the output and undoes it", async () => {
    const result = runResult({ tool: "pdf.delete_pages", metrics: { page_count: 3 } });
    const client = fakeClient({ runTool: vi.fn(async () => result) });
    const { input } = await open(client);
    await drop(input, pdfFile("a.pdf", 2048));
    await screen.findByText("What is in this PDF");
    pickTool("pdf.delete_pages");
    fireEvent.change(screen.getByLabelText("pages"), { target: { value: "2" } });
    fireEvent.click(screen.getByRole("button", { name: "Run tool on this device" }));

    const card = await waitFor(() => {
      const found = document.querySelector('[data-slot="tool-result"]') as HTMLElement;
      expect(found).toBeTruthy();
      return found;
    });
    expect(client.runTool).toHaveBeenCalledWith(
      "pdf.delete_pages",
      { pages: "2" },
      { inputIds: ["slot-1"], budgetBytes: STORE_INPUT_LIMIT_BYTES },
    );
    expect(within(card).getByText("(−75%)")).toBeTruthy();

    // The output becomes the workspace's input.
    await act(async () => {
      fireEvent.click(within(card).getByRole("button", { name: "Continue with another tool" }));
    });
    await waitFor(() => expect(document.querySelector('[data-slot="tool-result"]')).toBeNull());
    expect(ingested(client, 1)).toEqual(["report.pdf"]);
    expect(
      within(screen.getByRole("list", { name: "Files in this workspace" })).getByText("report.pdf"),
    ).toBeTruthy();
    expect(within(screen.getByRole("list", { name: "Steps" })).getByText("Done")).toBeTruthy();

    // Undo restores the file the output replaced.
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    });
    await waitFor(() => expect(client.ingestMany).toHaveBeenCalledTimes(3));
    expect(ingested(client, 2)).toEqual(["a.pdf"]);
    await waitFor(() =>
      expect(
        within(screen.getByRole("list", { name: "Files in this workspace" })).getByText("a.pdf"),
      ).toBeTruthy(),
    );
    expect(screen.queryByRole("button", { name: "Undo" })).toBeNull();
  });

  it("undoing a shown result discards it and keeps the input", async () => {
    const { input, client } = await open();
    await drop(input, pdfFile("a.pdf"));
    await screen.findByText("What is in this PDF");
    pickTool("pdf.rotate");
    fireEvent.change(screen.getByLabelText("angle"), { target: { value: "90" } });
    fireEvent.click(screen.getByRole("button", { name: "Run tool on this device" }));
    const card = await resultCard();
    await act(async () => {
      fireEvent.click(within(card).getByRole("button", { name: "Undo" }));
    });
    expect(document.querySelector('[data-slot="tool-result"]')).toBeNull();
    expect(client.ingestMany).toHaveBeenCalledTimes(1);
  });

  it("applies page edits as a chain, each step reading the last output, and restores the file", async () => {
    const client = fakeClient({
      runTool: vi.fn(async (toolId: string) => runResult({ tool: toolId })),
    });
    const { input } = await open(client);
    await drop(input, pdfFile("a.pdf"));
    await screen.findByText("What is in this PDF");
    fireEvent.mouseDown(screen.getByRole("tab", { name: "Pages" }));
    fireEvent.click(screen.getByRole("tab", { name: "Pages" }));
    await screen.findByRole("button", { name: "Rotate page 1" });
    // Previews come from the backend's preview source.
    expect(screen.getByTestId("preview-1")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Rotate page 1" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete page 4" }));
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Apply changes" }));
    });
    await waitFor(() => expect(document.querySelector('[data-slot="tool-result"]')).toBeTruthy());

    const calls = client.runTool.mock.calls as unknown as Array<
      [string, Record<string, unknown>, { inputIds: string[] }]
    >;
    expect(calls.map((call) => call[0])).toEqual(["pdf.rotate", "pdf.delete_pages"]);
    expect(calls[0]?.[1]).toEqual({ angle: 90, pages: "1" });
    expect(calls[1]?.[1]).toEqual({ pages: "4" });
    expect(calls[0]?.[2].inputIds).toEqual(["slot-1"]);
    // The second step read the first step's output through a fresh slot.
    expect(calls[1]?.[2].inputIds).toEqual(["slot-2"]);
    expect(ingested(client, 1)[0]).toBe("report.pdf");
    // And the original file is back in the store afterwards.
    expect(ingested(client, 2)).toEqual(["a.pdf"]);
  });

  it("cancels a run, then puts the file back because the worker was replaced", async () => {
    let reject: (reason: unknown) => void = () => undefined;
    const client = fakeClient({
      runTool: vi.fn(
        () =>
          new Promise<never>((_, fail) => {
            reject = fail;
          }),
      ),
    });
    const { input } = await open(client);
    await drop(input, pdfFile("a.pdf"));
    await screen.findByText("What is in this PDF");
    pickTool("pdf.rotate");
    fireEvent.change(screen.getByLabelText("angle"), { target: { value: "90" } });
    fireEvent.click(screen.getByRole("button", { name: "Run tool on this device" }));
    await screen.findByRole("button", { name: "Cancel" });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(client.cancel).toHaveBeenCalled();
    await act(async () => reject({ code: "cancelled", message: "The conversion was cancelled." }));
    await waitFor(() => expect(client.ingestMany).toHaveBeenCalledTimes(2));
    expect(screen.queryByRole("alert")).toBeNull();
    expect(screen.getByRole("button", { name: "Run tool on this device" })).toBeTruthy();
  });

  it("shows a failed run and marks the step", async () => {
    const client = fakeClient({
      runTool: vi.fn().mockRejectedValue({ code: "invalid_params", message: "angle must be 90" }),
    });
    const { input } = await open(client);
    await drop(input, pdfFile("a.pdf"));
    await screen.findByText("What is in this PDF");
    pickTool("pdf.rotate");
    fireEvent.change(screen.getByLabelText("angle"), { target: { value: "90" } });
    fireEvent.click(screen.getByRole("button", { name: "Run tool on this device" }));
    expect(
      (await screen.findByText(/The tool stopped: angle must be 90/)).getAttribute("role"),
    ).toBe("alert");
    expect(within(screen.getByRole("list", { name: "Steps" })).getByText("Failed")).toBeTruthy();
  });

  it("adds files to the set, keeps their order for multi-file tools, and removes one", async () => {
    const client = fakeClient();
    const { input } = await open(client);
    await drop(input, pdfFile("a.pdf"));
    await screen.findByText("What is in this PDF");
    // Adding re-ingests the whole set so tool inputs stay consistent.
    await act(async () => {
      fireEvent.change(document.querySelectorAll("input[type=file]")[0] as HTMLInputElement, {
        target: { files: [pdfFile("b.pdf")] },
      });
    });
    await waitFor(() => expect(client.ingestMany).toHaveBeenCalledTimes(2));
    expect(ingested(client, 1)).toEqual(["a.pdf", "b.pdf"]);

    await waitFor(() => expect(screen.getByRole("button", { name: "Move b.pdf up" })).toBeTruthy());
    fireEvent.click(screen.getByRole("button", { name: "Move b.pdf up" }));
    pickTool("pdf.merge");
    fireEvent.click(screen.getByRole("button", { name: "Run tool on this device" }));
    await waitFor(() => expect(client.runTool).toHaveBeenCalled());
    const ids = (
      client.runTool.mock.calls[0] as unknown as [string, unknown, { inputIds: string[] }]
    )[2].inputIds;
    expect(ids).toHaveLength(2);

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Remove a.pdf" }));
    });
    await waitFor(() => expect(client.ingestMany).toHaveBeenCalledTimes(3));
    expect(ingested(client, 2)).toEqual(["b.pdf"]);
  });

  it("shows an ingestion failure", async () => {
    const client = fakeClient({
      ingestMany: vi.fn().mockRejectedValue({ code: "limit_exceeded", message: "Too big" }),
    });
    const { input } = await open(client);
    await drop(input, pdfFile("huge.pdf"));
    // No file was accepted, so the drop zone is still there for another try.
    await waitFor(() => expect(screen.queryByText("Reading files…")).toBeNull());
    expect(screen.getByText(/Nothing is uploaded/)).toBeTruthy();
  });

  it("offers the editor and recipe actions only when the host provides them", async () => {
    const onOpenInEditor = vi.fn();
    const onSaveRecipe = vi.fn();
    const { input } = await open(fakeClient(), { onOpenInEditor, onSaveRecipe });
    await drop(input, pdfFile("a.pdf"));
    await screen.findByText("What is in this PDF");
    pickTool("pdf.rotate");
    fireEvent.change(screen.getByLabelText("angle"), { target: { value: "90" } });
    fireEvent.click(screen.getByRole("button", { name: "Run tool on this device" }));
    const card = await resultCard();
    fireEvent.click(within(card).getByRole("button", { name: "Save as recipe" }));
    expect(onSaveRecipe).toHaveBeenCalledWith([
      { toolId: "pdf.rotate", params: { angle: 90, pages: "all" } },
    ]);
    fireEvent.click(within(card).getByRole("button", { name: "Open in editor" }));
    expect(onOpenInEditor).toHaveBeenCalledTimes(1);
  });
});
