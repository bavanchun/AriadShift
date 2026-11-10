// @vitest-environment jsdom

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import {
  IMPOSITION_TOOLS,
  ImpositionPanel,
  isImpositionTool,
} from "@/components/tools/panels/imposition";
import { bookletSheets, gridDimensions } from "@/components/tools/panels/imposition/paper";
import {
  isOrganiserTool,
  ORGANISER_TOOLS,
  OrganiserPanel,
} from "@/components/tools/panels/organiser";
import { interleave } from "@/components/tools/panels/organiser/multi-file-studio";
import { expandPageRange } from "@/components/tools/panels/organiser/page-range";
import { presetOrder } from "@/components/tools/panels/organiser/reorder-studio";
import { rangesFromSplitPoints } from "@/components/tools/panels/organiser/split-studio";
import type { CustomToolPanelProps } from "@/components/tools/panels/types";
import { type ParamsValidation, validateParams } from "@/components/tools/params-form";
import { tool, workspaceFile } from "./workspace-fixtures";

afterEach(cleanup);

type Emission = [Record<string, unknown>, ParamsValidation];

function setup(
  Panel: (props: CustomToolPanelProps) => React.ReactElement,
  id: string,
  options: { pageCount?: number; files?: ReturnType<typeof workspaceFile>[] } = {},
) {
  const onChange = vi.fn<(params: Record<string, unknown>, validation: ParamsValidation) => void>();
  const pageCount = options.pageCount ?? 12;
  const files = options.files ?? [workspaceFile("report.pdf", { pageCount })];
  const descriptor = tool(id);
  render(<Panel tool={descriptor} files={files} pageCount={pageCount} onChange={onChange} />);
  const last = (): Emission => {
    const call = onChange.mock.lastCall;
    if (!call) throw new Error("onChange was not called");
    return [call[0], call[1]];
  };
  /** The panel's verdict must agree with the registry schema validated on its own. */
  const lastChecked = (): Emission => {
    const [params, validation] = last();
    const schema = validateParams(
      descriptor.params_schema as Record<string, unknown>,
      params,
      pageCount,
    );
    expect(Object.keys(schema.errors).every((key) => key in validation.errors)).toBe(true);
    return [params, validation];
  };
  return { onChange, last, lastChecked };
}

const organiser = (id: string, options?: Parameters<typeof setup>[2]) =>
  setup(OrganiserPanel, id, options);
const imposition = (id: string, options?: Parameters<typeof setup>[2]) =>
  setup(ImpositionPanel, id, options);
const typeInto = (name: string | RegExp, value: string) =>
  fireEvent.change(screen.getByRole("textbox", { name }), { target: { value } });

describe("tool id lists", () => {
  it("covers every organiser and imposition tool the registry declares", () => {
    expect([...ORGANISER_TOOLS]).toEqual([
      "pdf.merge",
      "pdf.split",
      "pdf.extract_pages",
      "pdf.delete_pages",
      "pdf.reorder",
      "pdf.rotate",
      "pdf.insert_blank",
      "pdf.alternate_mix",
    ]);
    expect([...IMPOSITION_TOOLS]).toEqual([
      "pdf.nup",
      "pdf.booklet",
      "pdf.scale",
      "pdf.single_page",
      "pdf.crop",
    ]);
    for (const id of [...ORGANISER_TOOLS, ...IMPOSITION_TOOLS]) expect(tool(id).id).toBe(id);
    expect(isOrganiserTool("pdf.rotate")).toBe(true);
    expect(isOrganiserTool("pdf.nup")).toBe(false);
    expect(isImpositionTool("pdf.crop")).toBe(true);
    expect(isImpositionTool("pdf.merge")).toBe(false);
  });
});

describe("page range helpers", () => {
  it("expands the shared grammar in selection order", () => {
    expect(expandPageRange("all", 4)).toEqual([1, 2, 3, 4]);
    expect(expandPageRange("odd", 5)).toEqual([1, 3, 5]);
    expect(expandPageRange("even", 5)).toEqual([2, 4]);
    expect(expandPageRange("2-", 5)).toEqual([2, 3, 4, 5]);
    expect(expandPageRange("-3", 5)).toEqual([1, 2, 3]);
    expect(expandPageRange("4-2, last, last-1", 5)).toEqual([4, 3, 2, 5, 4]);
    expect(expandPageRange("6", 5)).toBeNull();
    expect(expandPageRange("last-5", 5)).toBeNull();
    expect(expandPageRange("x", 5)).toBeNull();
    expect(expandPageRange("", 5)).toBeNull();
  });

  it("derives split ranges, reorder presets, interleaving and booklet sheets", () => {
    expect(rangesFromSplitPoints(12, new Set([3, 7]))).toEqual(["1-3", "4-7", "8-12"]);
    expect(rangesFromSplitPoints(5, new Set([1, 4]))).toEqual(["1", "2-4", "5"]);
    expect(presetOrder("reverse", 4)).toEqual([4, 3, 2, 1]);
    expect(presetOrder("odd-then-even", 5)).toEqual([1, 3, 5, 2, 4]);
    expect(presetOrder("duplex-interleave", 6)).toEqual([1, 6, 2, 5, 3, 4]);
    expect(interleave(3, 2, false).map((slot) => `${slot.from}${slot.page}`)).toEqual([
      "a1",
      "b1",
      "a2",
      "b2",
      "a3",
    ]);
    expect(interleave(2, 2, true).map((slot) => `${slot.from}${slot.page}`)).toEqual([
      "a1",
      "b2",
      "a2",
      "b1",
    ]);
    expect(bookletSheets(8, "left")).toEqual([
      { front: [8, 1], back: [2, 7] },
      { front: [6, 3], back: [4, 5] },
    ]);
    expect(bookletSheets(8, "right")[0]).toEqual({ front: [1, 8], back: [7, 2] });
    expect(gridDimensions(2, { w: 595, h: 842 })).toEqual({ cols: 1, rows: 2 });
    expect(gridDimensions(8, { w: 842, h: 595 })).toEqual({ cols: 4, rows: 2 });
  });
});

describe("<OrganiserPanel /> page selection (pdf.extract_pages, pdf.delete_pages)", () => {
  it("selects pages with quick chips and reports a live count", () => {
    const { lastChecked } = organiser("pdf.extract_pages");
    expect(lastChecked()[0]).toEqual({ pages: "1" });
    expect(screen.getByText("1 of 12 pages selected")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Odd pages" }));
    expect(lastChecked()).toEqual([{ pages: "odd" }, { isValid: true, errors: {} }]);
    expect(screen.getByText("6 of 12 pages selected")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Even pages" }));
    expect(lastChecked()[0]).toEqual({ pages: "even" });
    fireEvent.click(screen.getByRole("button", { name: "Last page" }));
    expect(lastChecked()[0]).toEqual({ pages: "last" });
    fireEvent.click(screen.getByRole("button", { name: "All pages" }));
    expect(screen.getByText("12 of 12 pages selected")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "First page" }));
    expect(lastChecked()[0]).toEqual({ pages: "1" });

    fireEvent.click(screen.getByRole("button", { name: "Custom range" }));
    typeInto("Page range", "2-4, last");
    expect(lastChecked()[0]).toEqual({ pages: "2-4, last" });
    expect(screen.getByText("4 of 12 pages selected")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Custom range" }).getAttribute("aria-pressed")).toBe(
      "true",
    );
  });

  it("rejects ranges outside the document and malformed text", () => {
    const { last } = organiser("pdf.extract_pages");
    typeInto("Page range", "3-99");
    expect(last()[1].isValid).toBe(false);
    expect(last()[1].errors.pages).toMatch(/exceeds page count/i);
    expect(screen.getByRole("alert").textContent).toMatch(/exceeds page count/i);
    expect(screen.queryByText(/pages selected/)).toBeNull();

    typeInto("Page range", "abc");
    expect(last()[1].isValid).toBe(false);
    typeInto("Page range", "");
    expect(last()[1].isValid).toBe(false);
  });

  it("refuses to delete every page or list a page twice", () => {
    const { last } = organiser("pdf.delete_pages");
    expect(last()[1].isValid).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "All pages" }));
    expect(last()[1].isValid).toBe(false);
    expect(last()[1].errors.pages).toBe("At least one page must remain.");
    typeInto("Page range", "2, 2");
    expect(last()[1].errors.pages).toBe("Each page can be listed only once.");
    typeInto("Page range", "2-3");
    expect(last()).toEqual([{ pages: "2-3" }, { isValid: true, errors: {} }]);
  });
});

describe("<OrganiserPanel /> pdf.rotate", () => {
  it("offers three angles with a rotated page preview", () => {
    const { lastChecked } = organiser("pdf.rotate");
    expect(lastChecked()).toEqual([
      { angle: 90, pages: "all" },
      { isValid: true, errors: {} },
    ]);
    const preview = screen.getByRole("img", { name: "Preview of a page rotated 90 degrees" });
    expect(preview.querySelector('[data-rotation="90"]')).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "90° CCW" }));
    expect(lastChecked()[0]).toEqual({ angle: -90, pages: "all" });
    expect(
      screen
        .getByRole("img", { name: "Preview of a page rotated -90 degrees" })
        .querySelector('[data-rotation="-90"]'),
    ).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "180°" }));
    expect(lastChecked()[0]).toEqual({ angle: 180, pages: "all" });

    fireEvent.click(screen.getByRole("button", { name: "Odd pages" }));
    expect(lastChecked()[0]).toEqual({ angle: 180, pages: "odd" });
  });

  it("flags repeated and out-of-range pages", () => {
    const { last } = organiser("pdf.rotate", { pageCount: 4 });
    typeInto("Page range", "1, 1");
    expect(last()[1].isValid).toBe(false);
    typeInto("Page range", "9");
    expect(last()[1].isValid).toBe(false);
  });
});

describe("<OrganiserPanel /> pdf.split", () => {
  it("splits by ranges and counts the PDFs it creates", () => {
    const { lastChecked } = organiser("pdf.split");
    expect(lastChecked()).toEqual([
      { mode: "ranges", ranges: ["1-6", "7-last"] },
      { isValid: true, errors: {} },
    ]);
    expect(screen.getByText("Creates 2 PDFs")).toBeTruthy();
    typeInto(/Ranges/, "1-2, 3, 4-last");
    expect(lastChecked()[0]).toEqual({ mode: "ranges", ranges: ["1-2", "3", "4-last"] });
    expect(screen.getByText("Creates 3 PDFs")).toBeTruthy();
    typeInto(/Ranges/, "0-3");
    expect(lastChecked()[1].isValid).toBe(false);
    typeInto(/Ranges/, "");
    expect(lastChecked()[1].isValid).toBe(false);
  });

  it("splits every N pages", () => {
    const { lastChecked } = organiser("pdf.split");
    fireEvent.click(screen.getByRole("button", { name: "Every N pages" }));
    expect(lastChecked()[0]).toEqual({ mode: "every", every: 1 });
    expect(screen.getByText("Creates 12 PDFs")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Pages per part"), { target: { value: "5" } });
    expect(lastChecked()).toEqual([
      { mode: "every", every: 5 },
      { isValid: true, errors: {} },
    ]);
    expect(screen.getByText("Creates 3 PDFs")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Pages per part"), { target: { value: "0" } });
    expect(lastChecked()[1].isValid).toBe(false);
    expect(screen.getByRole("alert").textContent).toBe("Enter a whole number of at least 1.");
  });

  it("splits at visual markers between page pills", () => {
    const { lastChecked } = organiser("pdf.split");
    fireEvent.click(screen.getByRole("button", { name: "Split points" }));
    expect(lastChecked()[0]).toEqual({ mode: "ranges", ranges: ["1-12"] });
    expect(screen.getByText("Creates 1 PDF")).toBeTruthy();
    const marker = (page: number) =>
      screen.getByRole("button", { name: `Split after page ${page}` });
    expect(screen.getAllByRole("button", { name: /Split after page/ })).toHaveLength(11);
    fireEvent.click(marker(3));
    fireEvent.click(marker(7));
    expect(lastChecked()[0]).toEqual({ mode: "ranges", ranges: ["1-3", "4-7", "8-12"] });
    expect(marker(3).getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByText("Creates 3 PDFs")).toBeTruthy();
    fireEvent.click(marker(3));
    expect(lastChecked()[0]).toEqual({ mode: "ranges", ranges: ["1-7", "8-12"] });
  });

  it("falls back to ranges for very long documents", () => {
    const { last } = organiser("pdf.split", { pageCount: 250 });
    fireEvent.click(screen.getByRole("button", { name: "Split points" }));
    expect(last()[1].isValid).toBe(false);
    expect(screen.queryAllByRole("button", { name: /Split after page/ })).toHaveLength(0);
    expect(screen.getByRole("alert").textContent).toMatch(/over 200 pages/);
  });
});

describe("<OrganiserPanel /> pdf.insert_blank", () => {
  it("chooses where, how many and the blank size, and previews the result", () => {
    const { lastChecked } = organiser("pdf.insert_blank", { pageCount: 4 });
    expect(lastChecked()).toEqual([
      { after: "all", count: 1, size: "same" },
      { isValid: true, errors: {} },
    ]);
    expect(screen.getByText("4 → 8 pages")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Odd pages" }));
    fireEvent.change(screen.getByLabelText("Blank pages at each position"), {
      target: { value: "2" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Letter" }));
    expect(lastChecked()[0]).toEqual({ after: "odd", count: 2, size: "Letter" });
    expect(screen.getByText("4 → 8 pages")).toBeTruthy();
    const strip = document.querySelector('[data-slot="blank-preview"]');
    expect(strip?.querySelectorAll('[data-tone="blank"]')).toHaveLength(4);

    fireEvent.click(screen.getByRole("button", { name: "A4" }));
    fireEvent.click(screen.getByRole("button", { name: "Legal" }));
    fireEvent.click(screen.getByRole("button", { name: "Match source" }));
    expect(lastChecked()[0]).toEqual({ after: "odd", count: 2, size: "same" });
  });

  it("rejects a zero count and an out-of-range position", () => {
    const { last } = organiser("pdf.insert_blank", { pageCount: 4 });
    fireEvent.change(screen.getByLabelText("Blank pages at each position"), {
      target: { value: "0" },
    });
    expect(last()[1].isValid).toBe(false);
    expect(last()[1].errors.count).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Blank pages at each position"), {
      target: { value: "1" },
    });
    typeInto("Insert after pages", "9");
    expect(last()[1].isValid).toBe(false);
    expect(last()[1].errors.after).toBeTruthy();
  });
});

describe("<OrganiserPanel /> pdf.reorder", () => {
  it("applies presets and shows the new order", () => {
    const { lastChecked } = organiser("pdf.reorder", { pageCount: 6 });
    expect(lastChecked()).toEqual([{ order: "reverse" }, { isValid: true, errors: {} }]);
    const order = () =>
      within(screen.getByRole("list", { name: "New order" }))
        .getAllByRole("listitem")
        .map((item) => item.textContent);
    expect(order()).toEqual(["6", "5", "4", "3", "2", "1"]);
    fireEvent.click(screen.getByRole("button", { name: "Duplex interleave" }));
    expect(lastChecked()[0]).toEqual({ order: "duplex-interleave" });
    expect(order()).toEqual(["1", "6", "2", "5", "3", "4"]);
    fireEvent.click(screen.getByRole("button", { name: "Odd then even" }));
    expect(lastChecked()[0]).toEqual({ order: "odd-then-even" });
    expect(order()).toEqual(["1", "3", "5", "2", "4", "6"]);
  });

  it("accepts a custom order and rejects an invalid one", () => {
    const { last } = organiser("pdf.reorder", { pageCount: 6 });
    fireEvent.click(screen.getByRole("button", { name: "Custom order" }));
    expect(last()[1].isValid).toBe(false);
    typeInto("Custom page order", "3, 1, 2-last");
    expect(last()).toEqual([{ order: "3, 1, 2-last" }, { isValid: true, errors: {} }]);
    expect(
      within(screen.getByRole("list", { name: "New order" }))
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["3", "1", "2", "3", "4", "5", "6"]);
    typeInto("Custom page order", "7");
    expect(last()[1].isValid).toBe(false);
  });
});

describe("<OrganiserPanel /> pdf.merge", () => {
  const files = [
    workspaceFile("a.pdf", { pageCount: 3 }),
    workspaceFile("b.pdf", { pageCount: 5 }),
  ];

  it("shows the merge order and sets bookmark and page-label handling", () => {
    const { lastChecked } = organiser("pdf.merge", { files });
    expect(lastChecked()).toEqual([
      { outline: "per-file", page_labels: "renumber" },
      { isValid: true, errors: {} },
    ]);
    const list = screen.getByRole("list", { name: "Merge order" });
    expect(
      within(list)
        .getAllByRole("listitem")
        .map((item) => item.textContent),
    ).toEqual(["1a.pdf3 pages", "2b.pdf5 pages"]);
    expect(screen.getByText("2 files, 8 pages")).toBeTruthy();
    fireEvent.change(screen.getByLabelText("Bookmarks"), { target: { value: "none" } });
    fireEvent.change(screen.getByLabelText("Page labels"), { target: { value: "keep" } });
    expect(lastChecked()[0]).toEqual({ outline: "none", page_labels: "keep" });
  });

  it("needs at least two files", () => {
    const { last } = organiser("pdf.merge", { files: files.slice(0, 1) });
    expect(last()[1].isValid).toBe(false);
    expect(last()[1].errors.inputs).toBe("Add at least 2 PDFs to continue.");
    expect(screen.getByRole("alert").textContent).toBe("Add at least 2 PDFs to continue.");
  });
});

describe("<OrganiserPanel /> pdf.alternate_mix", () => {
  const files = [
    workspaceFile("front.pdf", { pageCount: 3 }),
    workspaceFile("back.pdf", { pageCount: 2 }),
  ];
  const diagram = () =>
    within(screen.getByRole("list", { name: "Interleave preview" }))
      .getAllByRole("listitem")
      .map((item) => item.textContent)
      .join(" ");

  it("draws the A/B alternation and honours reverse_second", () => {
    const { lastChecked } = organiser("pdf.alternate_mix", { files });
    expect(lastChecked()).toEqual([{ reverse_second: false }, { isValid: true, errors: {} }]);
    expect(diagram()).toBe("A1 B1 A2 B2 A3");
    expect(screen.getByText("front.pdf")).toBeTruthy();
    expect(screen.getByText("back.pdf")).toBeTruthy();

    fireEvent.click(screen.getByLabelText("Take the second file in reverse order"));
    expect(lastChecked()[0]).toEqual({ reverse_second: true });
    expect(diagram()).toBe("A1 B2 A2 B1 A3");
  });

  it("requires exactly two files and marks an unknown page count as an example", () => {
    const { last } = organiser("pdf.alternate_mix", { files: files.slice(0, 1) });
    expect(last()[1].isValid).toBe(false);
    cleanup();
    const three = organiser("pdf.alternate_mix", {
      files: [...files, workspaceFile("extra.pdf", { pageCount: 1 })],
    });
    expect(three.last()[1].errors.inputs).toBe("This tool accepts at most 2 PDFs.");
    cleanup();
    organiser("pdf.alternate_mix", {
      files: [
        workspaceFile("a.pdf", { pageCount: null }),
        workspaceFile("b.pdf", { pageCount: null }),
      ],
    });
    expect(screen.getByText("Example with 4 pages in each file.")).toBeTruthy();
  });
});

describe("<ImpositionPanel /> pdf.nup", () => {
  const cell = (page: number) =>
    document.querySelector(`[data-slot="nup-cell"][data-page="${page}"]`);
  const preview = () => document.querySelector('[data-slot="nup-preview"]');

  it("emits schema keys for every grid preset and draws numbered cells", () => {
    const { lastChecked } = imposition("pdf.nup");
    expect(lastChecked()).toEqual([
      { per_sheet: 2, order: "row", borders: false, margins_pt: 0, sheet: "A4" },
      { isValid: true, errors: {} },
    ]);
    expect(screen.getByText("12 pages → 6 sheets")).toBeTruthy();
    for (const [perSheet, cols, rows] of [
      [2, 1, 2],
      [4, 2, 2],
      [6, 2, 3],
      [8, 2, 4],
      [9, 3, 3],
      [16, 4, 4],
    ] as const) {
      fireEvent.click(screen.getByRole("button", { name: String(perSheet) }));
      expect(lastChecked()[0].per_sheet).toBe(perSheet);
      expect(preview()?.getAttribute("data-cols")).toBe(String(cols));
      expect(preview()?.getAttribute("data-rows")).toBe(String(rows));
      expect(preview()?.querySelectorAll('[data-slot="nup-cell"]')).toHaveLength(perSheet);
      expect(cell(perSheet)?.textContent).toBe(String(perSheet));
    }
    expect(screen.getByRole("img", { name: "16-up sheet layout preview" })).toBeTruthy();
  });

  it("changes order, sheet, orientation, borders and margins", () => {
    const { lastChecked } = imposition("pdf.nup");
    fireEvent.click(screen.getByRole("button", { name: "4" }));
    const position = (page: number) => {
      const rect = cell(page)?.querySelector("rect");
      return { x: Number(rect?.getAttribute("x")), y: Number(rect?.getAttribute("y")) };
    };
    expect(position(2).x).toBeGreaterThan(position(1).x);
    expect(position(2).y).toBe(position(1).y);
    fireEvent.click(screen.getByRole("button", { name: "Down, then across" }));
    expect(lastChecked()[0].order).toBe("column");
    expect(position(2).x).toBe(position(1).x);
    expect(position(2).y).toBeGreaterThan(position(1).y);

    fireEvent.click(screen.getByRole("button", { name: "Letter" }));
    expect(lastChecked()[0].sheet).toBe("Letter");
    fireEvent.click(screen.getByLabelText("Draw a border around each page"));
    expect(lastChecked()[0].borders).toBe(true);
    fireEvent.change(screen.getByLabelText("Margins (pt)"), { target: { value: "18" } });
    expect(lastChecked()[0].margins_pt).toBe(18);

    fireEvent.click(screen.getByLabelText("Landscape sheet"));
    expect(lastChecked()[0].sheet).toEqual({ w: 792, h: 612 });
    fireEvent.click(screen.getByRole("button", { name: "6" }));
    expect(preview()?.getAttribute("data-cols")).toBe("3");
    expect(preview()?.getAttribute("data-rows")).toBe("2");
  });

  it("rejects margins that leave no room or are negative", () => {
    const { last } = imposition("pdf.nup");
    fireEvent.change(screen.getByLabelText("Margins (pt)"), { target: { value: "400" } });
    expect(last()[1].isValid).toBe(false);
    expect(last()[1].errors.margins_pt).toBe("Margins must leave room for the pages.");
    fireEvent.change(screen.getByLabelText("Margins (pt)"), { target: { value: "-4" } });
    expect(last()[1].isValid).toBe(false);
    fireEvent.change(screen.getByLabelText("Margins (pt)"), { target: { value: "" } });
    expect(last()[1].isValid).toBe(false);
  });
});

describe("<ImpositionPanel /> pdf.booklet", () => {
  const spread = (name: string, index = 0) =>
    screen
      .getAllByRole("img", { name: new RegExp(`^${name}:`) })
      [index]?.getAttribute("aria-label");

  it("previews the saddle-stitch signature for each binding edge", () => {
    const { lastChecked } = imposition("pdf.booklet", { pageCount: 8 });
    expect(lastChecked()).toEqual([
      { sheet: "A4", binding: "left", creep_pt: 0 },
      { isValid: true, errors: {} },
    ]);
    expect(screen.getByText("8 pages → 2 sheets, folded and stitched")).toBeTruthy();
    expect(spread("Front", 0)).toBe("Front: 8, 1");
    expect(spread("Back", 0)).toBe("Back: 2, 7");
    expect(spread("Front", 1)).toBe("Front: 6, 3");

    fireEvent.click(screen.getByRole("button", { name: "Right (RTL)" }));
    expect(lastChecked()[0].binding).toBe("right");
    expect(spread("Front", 0)).toBe("Front: 1, 8");
    expect(spread("Back", 0)).toBe("Back: 7, 2");

    fireEvent.click(screen.getByRole("button", { name: "Letter" }));
    fireEvent.change(screen.getByLabelText("Creep compensation (pt)"), {
      target: { value: "1.5" },
    });
    expect(lastChecked()[0]).toEqual({ sheet: "Letter", binding: "right", creep_pt: 1.5 });
  });

  it("pads to a multiple of four and rejects negative creep", () => {
    const { last } = imposition("pdf.booklet", { pageCount: 6 });
    expect(screen.getByText("6 pages → 2 sheets, folded and stitched")).toBeTruthy();
    expect(spread("Front", 0)).toBe("Front: Blank, 1");
    fireEvent.change(screen.getByLabelText("Creep compensation (pt)"), { target: { value: "-1" } });
    expect(last()[1].isValid).toBe(false);
    expect(last()[1].errors.creep_pt).toBeTruthy();
  });
});

describe("<ImpositionPanel /> pdf.scale", () => {
  it("picks a target paper and fit mode with live scale factors", () => {
    const { lastChecked } = imposition("pdf.scale");
    expect(lastChecked()).toEqual([
      { to: "A4", fit: "contain", pages: "all" },
      { isValid: true, errors: {} },
    ]);
    expect(screen.getByText("Content scaled to 100% × 100%")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Letter" }));
    expect(lastChecked()[0]).toEqual({ to: "Letter", fit: "contain", pages: "all" });
    expect(screen.getByText("Content scaled to 94% × 94%")).toBeTruthy();
    expect(
      screen.getByRole("img", { name: "Preview of a page scaled onto Letter (Fit)" }),
    ).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Fill" }));
    expect(lastChecked()[0].fit).toBe("cover");
    expect(screen.getByText("Content scaled to 103% × 103%")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Stretch" }));
    expect(lastChecked()[0].fit).toBe("stretch");
    expect(screen.getByText("Content scaled to 103% × 94%")).toBeTruthy();

    for (const paper of ["Legal", "A3", "A5"]) {
      fireEvent.click(screen.getByRole("button", { name: paper }));
      expect(lastChecked()[0].to).toBe(paper);
    }
  });

  it("validates the page selection", () => {
    const { last } = imposition("pdf.scale", { pageCount: 4 });
    fireEvent.click(screen.getByRole("button", { name: "Even pages" }));
    expect(last()[0].pages).toBe("even");
    typeInto("Page range", "5-9");
    expect(last()[1].isValid).toBe(false);
  });
});

describe("<ImpositionPanel /> pdf.single_page", () => {
  it("takes no parameters and previews the continuous strip", () => {
    const { lastChecked } = imposition("pdf.single_page", { pageCount: 4 });
    expect(lastChecked()).toEqual([{}, { isValid: true, errors: {} }]);
    expect(screen.getByText("4 pages → 1 continuous page")).toBeTruthy();
    const strip = screen.getByRole("img", { name: "Continuous strip preview" });
    expect(strip.querySelectorAll('[data-slot="strip-page"]')).toHaveLength(4);
  });

  it("caps the strip for long documents", () => {
    imposition("pdf.single_page", { pageCount: 40 });
    expect(document.querySelectorAll('[data-slot="strip-page"]')).toHaveLength(5);
    expect(screen.getByText("40 pages → 1 continuous page")).toBeTruthy();
  });
});

describe("<ImpositionPanel /> pdf.crop", () => {
  const slider = (side: string) => screen.getByRole("slider", { name: `${side} margin` });
  const fullPage = { left: 0, bottom: 0, right: 595.28, top: 841.89 };

  it("starts from the whole page and applies margin presets", () => {
    const { lastChecked } = imposition("pdf.crop");
    expect(lastChecked()).toEqual([
      { box: fullPage, pages: "all", target: "crop" },
      { isValid: true, errors: {} },
    ]);
    expect(screen.getByText("Cropped size: 595.3 × 841.9 pt (210 × 297 mm)")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "0.5 in" }));
    expect(lastChecked()[0]).toEqual({
      box: { left: 36, bottom: 36, right: 559.28, top: 805.89 },
      pages: "all",
      target: "crop",
    });
    expect(screen.getByText(/Cropped size: 523\.3 × 769\.9 pt/)).toBeTruthy();
    expect(slider("Left").getAttribute("aria-valuenow")).toBe("36");

    fireEvent.click(screen.getByRole("button", { name: "No trim" }));
    expect(lastChecked()[0].box).toEqual(fullPage);
  });

  it("moves an edge with the keyboard and edits margins numerically", () => {
    const { lastChecked } = imposition("pdf.crop");
    fireEvent.keyDown(slider("Left"), { key: "ArrowRight" });
    expect((lastChecked()[0].box as Record<string, number>).left).toBe(1);
    fireEvent.keyDown(slider("Left"), { key: "ArrowRight", shiftKey: true });
    expect((lastChecked()[0].box as Record<string, number>).left).toBe(11);
    fireEvent.keyDown(slider("Left"), { key: "ArrowLeft" });
    expect((lastChecked()[0].box as Record<string, number>).left).toBe(10);
    fireEvent.keyDown(slider("Top"), { key: "ArrowDown", shiftKey: true });
    expect((lastChecked()[0].box as Record<string, number>).top).toBe(831.89);
    fireEvent.keyDown(slider("Bottom"), { key: "ArrowUp" });
    expect((lastChecked()[0].box as Record<string, number>).bottom).toBe(1);
    fireEvent.keyDown(slider("Right"), { key: "ArrowLeft", shiftKey: true });
    expect((lastChecked()[0].box as Record<string, number>).right).toBe(585.28);
    fireEvent.keyDown(slider("Left"), { key: "Tab" });

    fireEvent.change(screen.getByRole("spinbutton", { name: "Right margin" }), {
      target: { value: "20" },
    });
    expect((lastChecked()[0].box as Record<string, number>).right).toBe(575.28);
  });

  it("moves an edge by dragging it across the preview", () => {
    const { lastChecked } = imposition("pdf.crop");
    const svg = screen.getByRole("group", { name: "Crop box preview" });
    svg.getBoundingClientRect = () =>
      ({ left: 0, top: 0, width: 220, height: 312.6, right: 220, bottom: 312.6 }) as DOMRect;
    fireEvent.pointerDown(slider("Left"), { clientX: 0, clientY: 100 });
    fireEvent.pointerMove(svg, { clientX: 22, clientY: 100 });
    expect((lastChecked()[0].box as Record<string, number>).left).toBe(60);
    fireEvent.pointerUp(svg);
    fireEvent.pointerMove(svg, { clientX: 110, clientY: 100 });
    expect((lastChecked()[0].box as Record<string, number>).left).toBe(60);

    fireEvent.pointerDown(slider("Bottom"), { clientX: 100, clientY: 312 });
    fireEvent.pointerMove(svg, { clientX: 100, clientY: 281.34 });
    expect((lastChecked()[0].box as Record<string, number>).bottom).toBe(84);
    fireEvent.pointerLeave(svg);
  });

  it("keeps dragged edges inside the page and rejects empty or negative boxes", () => {
    const { last } = imposition("pdf.crop");
    const svg = screen.getByRole("group", { name: "Crop box preview" });
    svg.getBoundingClientRect = () =>
      ({ left: 0, top: 0, width: 220, height: 312.6, right: 220, bottom: 312.6 }) as DOMRect;
    fireEvent.change(screen.getByRole("spinbutton", { name: "Right margin" }), {
      target: { value: "100" },
    });
    fireEvent.pointerDown(slider("Left"), { clientX: 0, clientY: 0 });
    fireEvent.pointerMove(svg, { clientX: 5000, clientY: 0 });
    expect((last()[0].box as Record<string, number>).left).toBe(494.28);
    expect(last()[1].isValid).toBe(true);
    fireEvent.pointerUp(svg);

    fireEvent.change(screen.getByRole("spinbutton", { name: "Left margin" }), {
      target: { value: "-5" },
    });
    expect(last()[1].isValid).toBe(false);
    expect(last()[1].errors.box).toBe("Margins cannot be negative.");
    fireEvent.change(screen.getByRole("spinbutton", { name: "Left margin" }), {
      target: { value: "700" },
    });
    expect(last()[1].isValid).toBe(false);
    expect(last()[1].errors.box).toBe("The crop box would be empty.");
  });

  it("sets the media box on chosen pages of a chosen reference size", () => {
    const { lastChecked, last } = imposition("pdf.crop", { pageCount: 4 });
    fireEvent.click(screen.getByRole("button", { name: "Letter" }));
    fireEvent.click(screen.getByRole("button", { name: "Media box" }));
    fireEvent.click(screen.getByRole("button", { name: "Odd pages" }));
    expect(lastChecked()[0]).toEqual({
      box: { left: 0, bottom: 0, right: 612, top: 792 },
      pages: "odd",
      target: "media",
    });
    typeInto("Page range", "7");
    expect(last()[1].isValid).toBe(false);
    expect(last()[1].errors.pages).toBeTruthy();
  });
});

describe("shared behaviour", () => {
  it("disables every control while a run is busy", () => {
    const onChange = vi.fn();
    render(
      <OrganiserPanel
        tool={tool("pdf.rotate")}
        files={[workspaceFile("a.pdf")]}
        pageCount={4}
        disabled
        onChange={onChange}
      />,
    );
    for (const button of screen.getAllByRole("button"))
      expect((button as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByRole("textbox", { name: "Page range" }) as HTMLInputElement).disabled).toBe(
      true,
    );
  });

  it("renders inside a right-to-left tree", () => {
    const onChange = vi.fn();
    render(
      <div dir="rtl">
        <ImpositionPanel
          tool={tool("pdf.booklet")}
          files={[workspaceFile("a.pdf", { pageCount: 8 })]}
          pageCount={8}
          locale="ar"
          onChange={onChange}
        />
      </div>,
    );
    expect(screen.getByRole("button", { name: "Right (RTL)" })).toBeTruthy();
    expect(onChange).toHaveBeenCalled();
  });

  it("restarts the draft when the tool changes", () => {
    const onChange = vi.fn();
    const files = [workspaceFile("a.pdf", { pageCount: 6 })];
    const { rerender } = render(
      <OrganiserPanel
        tool={tool("pdf.extract_pages")}
        files={files}
        pageCount={6}
        onChange={onChange}
      />,
    );
    typeInto("Page range", "2-3");
    rerender(
      <OrganiserPanel
        tool={tool("pdf.delete_pages")}
        files={files}
        pageCount={6}
        onChange={onChange}
      />,
    );
    expect((screen.getByRole("textbox", { name: "Page range" }) as HTMLInputElement).value).toBe(
      "1",
    );
  });

  it("uses logical layout properties only, so the panels mirror in RTL", () => {
    const root = path.join(import.meta.dirname, "../src/components/tools/panels");
    const physical =
      /\b(?:-?m[lr]|p[lr]|border-[lr]|rounded-[lr]|left|right)-(?:\d|\[|px|auto)|\btext-(?:left|right)\b/;
    const files = ["organiser", "imposition"].flatMap((dir) =>
      readdirSync(path.join(root, dir))
        .filter((name) => name.endsWith(".tsx"))
        .map((name) => path.join(root, dir, name)),
    );
    expect(files.length).toBeGreaterThan(5);
    for (const file of files) {
      const offending = readFileSync(file, "utf8")
        .split("\n")
        .filter((line) => /className/.test(line) && physical.test(line));
      expect({ file: path.basename(file), offending }).toEqual({
        file: path.basename(file),
        offending: [],
      });
    }
  });
});
