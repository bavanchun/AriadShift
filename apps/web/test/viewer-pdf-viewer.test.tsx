// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { PdfViewer } from "@/viewer/pdf-viewer";
import type { PdfViewerSource } from "@/viewer/types";
import { fakeBackend, readySnapshot } from "./viewer-fixtures";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const source = (id = "doc-1"): PdfViewerSource => ({
  id,
  name: `${id}.pdf`,
  data: new Blob(["%PDF"]),
});

async function ready(snapshot = readySnapshot()) {
  const fake = fakeBackend(snapshot);
  render(<PdfViewer source={source()} loadBackend={fake.load} />);
  await screen.findByTestId("engine");
  await waitFor(() =>
    expect((screen.getByRole("button", { name: "Zoom in" }) as HTMLButtonElement).disabled).toBe(
      false,
    ),
  );
  return fake;
}

describe("<PdfViewer />", () => {
  it("shows a status and keeps the controls disabled until the engine reports a document", async () => {
    const fake = fakeBackend({ ...readySnapshot(), status: "loading" });
    render(<PdfViewer source={source()} loadBackend={fake.load} />);
    expect(screen.getByText("Opening the document on this device…").getAttribute("role")).toBe(
      "status",
    );
    expect((screen.getByRole("button", { name: "Zoom in" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
    await screen.findByTestId("engine");
    // The engine's own first report arrives in an effect; emit only after it.
    await waitFor(() => expect(fake.log.mounts).toBe(1));
    act(() => fake.controls.emit(readySnapshot()));
    await waitFor(() =>
      expect((screen.getByRole("button", { name: "Zoom in" }) as HTMLButtonElement).disabled).toBe(
        false,
      ),
    );
    expect(screen.queryByText(/Opening the document/)).toBeNull();
  });

  it("hands the engine an absolute same-origin PDFium URL", async () => {
    const fake = await ready();
    const url = new URL(fake.log.props.at(-1)?.wasmUrl ?? "");
    expect(url.origin).toBe(location.origin);
    expect(url.pathname).toBe("/wasm/pdfium/pdfium.wasm");
  });

  it("zooms in and out, fits width and page, and shows the percentage", async () => {
    const fake = await ready(readySnapshot({ zoomPercent: 125, zoomMode: "custom" }));
    expect(screen.getByText("125%")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Zoom in" }));
    fireEvent.click(screen.getByRole("button", { name: "Zoom out" }));
    fireEvent.click(screen.getByRole("button", { name: "Fit page" }));
    fireEvent.click(screen.getByRole("button", { name: "Fit width" }));
    expect(fake.commands.zoomIn).toHaveBeenCalledTimes(1);
    expect(fake.commands.zoomOut).toHaveBeenCalledTimes(1);
    expect(fake.commands.setZoomMode).toHaveBeenNthCalledWith(1, "fit-page");
    expect(fake.commands.setZoomMode).toHaveBeenNthCalledWith(2, "fit-width");
    act(() => fake.controls.emit(readySnapshot({ zoomPercent: 80, zoomMode: "fit-page" })));
    expect(screen.getByText("80%")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Fit page" }).getAttribute("aria-pressed")).toBe(
      "true",
    );
    expect(screen.getByRole("button", { name: "Fit width" }).getAttribute("aria-pressed")).toBe(
      "false",
    );
  });

  it("jumps to a typed page, clamped to the document", async () => {
    const fake = await ready(readySnapshot({ page: 2, pageCount: 10 }));
    const input = screen.getByLabelText("Page number") as HTMLInputElement;
    expect(input.value).toBe("2");
    expect(screen.getByText("of 10")).toBeTruthy();
    fireEvent.change(input, { target: { value: "7" } });
    fireEvent.submit(input.closest("form") as HTMLFormElement);
    expect(fake.commands.goToPage).toHaveBeenLastCalledWith(7);
    fireEvent.change(input, { target: { value: "99" } });
    fireEvent.submit(input.closest("form") as HTMLFormElement);
    expect(fake.commands.goToPage).toHaveBeenLastCalledWith(10);
    fireEvent.change(input, { target: { value: "0" } });
    fireEvent.submit(input.closest("form") as HTMLFormElement);
    expect(fake.commands.goToPage).toHaveBeenLastCalledWith(1);
    // The field follows the scroll position again once editing ends.
    act(() => fake.controls.emit(readySnapshot({ page: 5, pageCount: 10 })));
    expect((screen.getByLabelText("Page number") as HTMLInputElement).value).toBe("5");
  });

  it("scrolls to the page the host asks for, and reports pages only once it has landed", async () => {
    const fake = fakeBackend(readySnapshot({ page: 1 }));
    const onPageChange = vi.fn();
    const view = render(
      <PdfViewer
        source={source()}
        loadBackend={fake.load}
        initialPage={4}
        onPageChange={onPageChange}
      />,
    );
    await screen.findByTestId("engine");
    await waitFor(() => expect(fake.commands.goToPage).toHaveBeenCalledWith(4));
    // Page 1 is where the engine starts; it must not overwrite the page the host asked for.
    expect(onPageChange).not.toHaveBeenCalled();
    act(() => fake.controls.emit(readySnapshot({ page: 4 })));
    await waitFor(() => expect(onPageChange).toHaveBeenLastCalledWith(4));

    // A new request with a new nonce scrolls again; the same nonce does not.
    view.rerender(
      <PdfViewer
        source={source()}
        loadBackend={fake.load}
        pageRequest={{ page: 7, nonce: 1 }}
        onPageChange={onPageChange}
      />,
    );
    await waitFor(() => expect(fake.commands.goToPage).toHaveBeenLastCalledWith(7));
    const calls = vi.mocked(fake.commands.goToPage).mock.calls.length;
    view.rerender(
      <PdfViewer
        source={source()}
        loadBackend={fake.load}
        pageRequest={{ page: 7, nonce: 1 }}
        onPageChange={onPageChange}
      />,
    );
    expect(vi.mocked(fake.commands.goToPage).mock.calls.length).toBe(calls);
  });

  it("does not report a page the reader never moved to, only one reached by interacting", async () => {
    const fake = fakeBackend(readySnapshot({ page: 1 }));
    const onPageChange = vi.fn();
    render(<PdfViewer source={source()} loadBackend={fake.load} onPageChange={onPageChange} />);
    await screen.findByTestId("engine");
    // A layout settling on page 3 before anyone scrolled is not the reader's position.
    act(() => fake.controls.emit(readySnapshot({ page: 3 })));
    expect(onPageChange).not.toHaveBeenCalled();
    fireEvent.wheel(screen.getByTestId("engine"));
    await waitFor(() => expect(onPageChange).toHaveBeenLastCalledWith(3));
    act(() => fake.controls.emit(readySnapshot({ page: 2 })));
    await waitFor(() => expect(onPageChange).toHaveBeenLastCalledWith(2));
  });

  it("searches with progress, a cancel button, a match counter and navigation", async () => {
    const fake = await ready();
    const box = screen.getByLabelText("Search in document");
    expect((screen.getByRole("button", { name: "Search" }) as HTMLButtonElement).disabled).toBe(
      true,
    );
    fireEvent.change(box, { target: { value: "needle" } });
    fireEvent.click(screen.getByRole("button", { name: "Search" }));
    expect(fake.commands.search).toHaveBeenCalledWith("needle");

    act(() =>
      fake.controls.emit(
        readySnapshot({
          search: { query: "needle", running: true, progress: 0.4, total: 3, current: 0 },
        }),
      ),
    );
    expect(screen.getByText("Searching… 40%")).toBeTruthy();
    expect(screen.getByRole("progressbar", { name: "Searching… 40%" })).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Cancel search" }));
    expect(fake.commands.cancelSearch).toHaveBeenCalledTimes(1);

    act(() =>
      fake.controls.emit(
        readySnapshot({
          search: { query: "needle", running: false, progress: 1, total: 3, current: 1 },
        }),
      ),
    );
    expect(screen.getByText("2 of 3")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Next match" }));
    fireEvent.click(screen.getByRole("button", { name: "Previous match" }));
    expect(fake.commands.nextMatch).toHaveBeenCalledTimes(1);
    expect(fake.commands.previousMatch).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
    expect(fake.commands.clearSearch).toHaveBeenCalledTimes(1);
    expect((box as HTMLInputElement).value).toBe("");
  });

  it("says when nothing matched and disables match navigation", async () => {
    await ready(
      readySnapshot({
        search: { query: "zzz", running: false, progress: 1, total: 0, current: -1 },
      }),
    );
    expect(screen.getByText("No matches")).toBeTruthy();
    for (const name of ["Next match", "Previous match"])
      expect((screen.getByRole("button", { name }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("toggles dark pages and tells the engine", async () => {
    await ready();
    const toggle = screen.getByRole("button", { name: "Dark pages" });
    expect(toggle.getAttribute("aria-pressed")).toBe("false");
    expect(screen.getByTestId("engine").getAttribute("data-invert")).toBe("false");
    fireEvent.click(toggle);
    expect(toggle.getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByTestId("engine").getAttribute("data-invert")).toBe("true");
  });

  it("keeps the dark pages choice when another document is opened", async () => {
    const fake = fakeBackend();
    const { rerender } = render(<PdfViewer source={source("one")} loadBackend={fake.load} />);
    await screen.findByTestId("engine");
    fireEvent.click(screen.getByRole("button", { name: "Dark pages" }));
    rerender(<PdfViewer source={source("two")} loadBackend={fake.load} />);
    await waitFor(() =>
      expect(screen.getByTestId("engine").getAttribute("data-source")).toBe("two"),
    );
    expect(screen.getByTestId("engine").getAttribute("data-invert")).toBe("true");
  });

  it("closes the previous document when another is opened, and on unmount", async () => {
    const fake = fakeBackend();
    const { rerender, unmount } = render(
      <PdfViewer source={source("one")} loadBackend={fake.load} />,
    );
    await screen.findByTestId("engine");
    expect(fake.log.mounts).toBe(1);
    rerender(<PdfViewer source={source("two")} loadBackend={fake.load} />);
    await waitFor(() =>
      expect(screen.getByTestId("engine").getAttribute("data-source")).toBe("two"),
    );
    expect(fake.log.unmounts).toBe(1);
    expect(fake.log.mounts).toBe(2);
    // The new document's controls work: the old document's teardown did not clear them.
    await waitFor(() =>
      expect((screen.getByRole("button", { name: "Zoom in" }) as HTMLButtonElement).disabled).toBe(
        false,
      ),
    );
    fireEvent.click(screen.getByRole("button", { name: "Zoom in" }));
    expect(fake.commands.zoomIn).toHaveBeenCalledTimes(1);
    unmount();
    expect(fake.log.unmounts).toBe(2);
  });

  it("reports an engine that cannot load instead of an empty box", async () => {
    render(
      <PdfViewer
        source={source()}
        loadBackend={vi.fn().mockRejectedValue(new Error("chunk failed"))}
      />,
    );
    expect((await screen.findByRole("alert")).textContent).toContain("needs WebAssembly");
  });

  it("does not start an engine in a browser without WebAssembly", () => {
    vi.stubGlobal("WebAssembly", undefined);
    const load = vi.fn();
    render(<PdfViewer source={source()} loadBackend={load} />);
    expect(screen.getByRole("alert").textContent).toContain("needs WebAssembly");
    expect(load).not.toHaveBeenCalled();
  });

  it("reports a document the engine could not open", async () => {
    const fake = fakeBackend();
    render(<PdfViewer source={source()} loadBackend={fake.load} />);
    await screen.findByTestId("engine");
    await waitFor(() => expect(fake.log.mounts).toBe(1));
    act(() => fake.controls.emit({ ...readySnapshot(), status: "error", error: "document" }));
    expect((await screen.findByRole("alert")).textContent).toContain("could not be displayed");
  });

  it("takes translated labels", async () => {
    const fake = fakeBackend();
    render(<PdfViewer source={source()} loadBackend={fake.load} labels={{ zoomIn: "Phóng to" }} />);
    await screen.findByTestId("engine");
    expect(screen.getByRole("button", { name: "Phóng to" })).toBeTruthy();
  });
});
