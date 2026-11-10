// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { createPageEditStore } from "@/components/workspace/page-edits";
import { PageGrid, VIRTUALIZE_AFTER } from "@/components/workspace/page-grid";

afterEach(cleanup);

const card = (page: number) => screen.getByRole("group", { name: `Page ${page}` });
const order = () =>
  screen
    .getAllByRole("group")
    .map((element) =>
      Number(within(element).getByRole("checkbox").getAttribute("aria-label")?.match(/\d+/)?.[0]),
    );

describe("<PageGrid />", () => {
  it("rotates, deletes and restores pages, counts pending edits, and undoes and redoes them", () => {
    render(<PageGrid pageCount={4} onApply={vi.fn()} />);
    const apply = screen.getByRole("button", { name: "Apply changes" });
    expect((apply as HTMLButtonElement).disabled).toBe(true);
    expect(screen.getByText("0 pending changes")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Rotate page 2" }));
    expect(card(2).querySelector('[data-rotation="90"]')).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Delete page 3" }));
    expect(card(3).getAttribute("data-deleted")).toBe("true");
    expect(screen.getByText("2 pending changes")).toBeTruthy();
    expect((apply as HTMLButtonElement).disabled).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: "Restore page 3" }));
    expect(card(3).getAttribute("data-deleted")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(card(3).getAttribute("data-deleted")).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: "Redo" }));
    expect(card(3).getAttribute("data-deleted")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Reset" }));
    expect(screen.getByText("0 pending changes")).toBeTruthy();
    expect(card(2).querySelector('[data-rotation="0"]')).toBeTruthy();
  });

  it("reorders with the keyboard from the grip and announces the move", () => {
    render(<PageGrid pageCount={3} onApply={vi.fn()} />);
    fireEvent.keyDown(screen.getByRole("button", { name: "Reorder page 1" }), {
      key: "ArrowRight",
    });
    expect(order()).toEqual([2, 1, 3]);
    expect(screen.getByRole("status").textContent).toBe("Page 1 moved to position 2");
    fireEvent.keyDown(screen.getByRole("button", { name: "Reorder page 3" }), { key: "ArrowUp" });
    expect(order()).toEqual([2, 3, 1]);
  });

  it("selects with shift for a range and applies bulk rotation and deletion", () => {
    render(<PageGrid pageCount={5} onApply={vi.fn()} />);
    fireEvent.click(screen.getByRole("checkbox", { name: "Select page 2" }));
    fireEvent.click(screen.getByRole("checkbox", { name: "Select page 4" }), { shiftKey: true });
    for (const page of [2, 3, 4])
      expect(
        (screen.getByRole("checkbox", { name: `Select page ${page}` }) as HTMLInputElement).checked,
      ).toBe(true);
    expect(
      (screen.getByRole("checkbox", { name: "Select page 5" }) as HTMLInputElement).checked,
    ).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: "Rotate left" }));
    for (const page of [2, 3, 4])
      expect(card(page).querySelector('[data-rotation="270"]')).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Delete selected" }));
    for (const page of [2, 3, 4]) expect(card(page).getAttribute("data-deleted")).toBe("true");
    // Two bulk edits, however many pages each touched.
    expect(screen.getByText("2 pending changes")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Restore selected" }));
    expect(card(3).getAttribute("data-deleted")).toBeNull();
  });

  it("selects pages typed as a range", () => {
    render(<PageGrid pageCount={6} onApply={vi.fn()} />);
    fireEvent.change(screen.getByLabelText("Select pages", { selector: "input" }), {
      target: { value: "2-4" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Select pages" }));
    expect(
      (screen.getByRole("checkbox", { name: "Select page 3" }) as HTMLInputElement).checked,
    ).toBe(true);
    expect(
      (screen.getByRole("checkbox", { name: "Select page 5" }) as HTMLInputElement).checked,
    ).toBe(false);
  });

  it("applies the planned tool calls in order, then clears the pending edits", async () => {
    const onApply = vi.fn();
    render(<PageGrid pageCount={4} onApply={onApply} />);
    fireEvent.click(screen.getByRole("button", { name: "Rotate page 1" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete page 4" }));
    fireEvent.keyDown(screen.getByRole("button", { name: "Reorder page 2" }), {
      key: "ArrowRight",
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply changes" }));
    await screen.findByText("0 pending changes");
    expect(onApply).toHaveBeenCalledWith([
      { toolId: "pdf.rotate", params: { angle: 90, pages: "1" } },
      { toolId: "pdf.delete_pages", params: { pages: "4" } },
      { toolId: "pdf.reorder", params: { order: "1,3,2" } },
    ]);
  });

  it("keeps the edits and shows the reason when applying fails", async () => {
    render(
      <PageGrid pageCount={2} onApply={vi.fn().mockRejectedValue(new Error("engine said no"))} />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Rotate page 1" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply changes" }));
    expect((await screen.findByRole("alert")).textContent).toBe("engine said no");
    expect(screen.getByText("1 pending change")).toBeTruthy();
  });

  it("will not apply a document with every page deleted", () => {
    render(<PageGrid pageCount={2} onApply={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Select all" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete selected" }));
    expect(screen.getByRole("alert").textContent).toBe("A document needs at least one page.");
    expect(
      (screen.getByRole("button", { name: "Apply changes" }) as HTMLButtonElement).disabled,
    ).toBe(true);
  });

  it("shows blank and scanned flags and renders the supplied preview", () => {
    render(
      <PageGrid
        pageCount={3}
        blankPages={[2]}
        scannedPages={[3]}
        renderPreview={(page) => <span data-testid={`preview-${page}`}>p{page}</span>}
        onApply={vi.fn()}
      />,
    );
    expect(within(card(2)).getByText("Blank")).toBeTruthy();
    expect(within(card(3)).getByText("Scanned")).toBeTruthy();
    expect(screen.getByTestId("preview-1").textContent).toBe("p1");
  });

  it("uses a supplied store, so a parent can observe edits", () => {
    const store = createPageEditStore(3);
    render(<PageGrid pageCount={3} store={store} onApply={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "Delete page 2" }));
    expect(store.getState().pages[1]?.deleted).toBe(true);
  });

  it("mounts only the rows near the viewport for a long document", () => {
    render(
      <PageGrid
        pageCount={VIRTUALIZE_AFTER + 380}
        initialRect={{ width: 960, height: 600 }}
        onApply={vi.fn()}
      />,
    );
    const mounted = screen.getAllByRole("group").length;
    expect(mounted).toBeGreaterThan(0);
    expect(mounted).toBeLessThan(120);
    expect(screen.queryByRole("group", { name: "Page 500" })).toBeNull();
  });
});
