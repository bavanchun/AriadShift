// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createElement, useState } from "react";
import { ToolCommandPalette, type ToolCommandPaletteProps } from "@/components/tools/palette";
import { toolCatalog } from "@/converter/tool-catalog";
import {
  FAVORITES_KEY,
  parseNaturalLanguageRecipe,
  RECENTS_KEY,
  recordRecent,
  toggleFavorite,
} from "@/lib/tools";

// jsdom lacks the browser APIs that floating and range components probe for.
globalThis.ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};
Element.prototype.scrollIntoView ??= () => {};
Element.prototype.getAnimations ??= () => [];
window.matchMedia ??= ((query: string) => ({
  matches: false,
  media: query,
  onchange: null,
  addEventListener() {},
  removeEventListener() {},
  addListener() {},
  removeListener() {},
  dispatchEvent: () => false,
})) as typeof window.matchMedia;

// Opening the palette mounts one row per tool, which is slow in jsdom.
vi.setConfig({ testTimeout: 30_000 });

const catalog = toolCatalog();

/** The palette with its own open state, the way a host page uses it. */
function Host(props: Partial<ToolCommandPaletteProps>) {
  const [open, setOpen] = useState(false);
  return createElement(ToolCommandPalette, {
    catalog,
    open,
    onOpenChange: setOpen,
    onSelectTool: () => {},
    ...props,
  });
}

const press = (init: KeyboardEventInit) => fireEvent.keyDown(window, { key: "k", ...init });
const input = () => screen.getByRole("combobox") as HTMLInputElement;
const type = (text: string) => fireEvent.change(input(), { target: { value: text } });
const options = () => screen.getAllByRole("option");
const optionIds = () => options().map((option) => option.getAttribute("data-tool-id"));
const closed = () => waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

beforeEach(() => {
  window.localStorage.clear();
  document.documentElement.classList.remove("dark");
});
afterEach(() => {
  cleanup();
  window.localStorage.clear();
  vi.restoreAllMocks();
});

describe("shortcut", () => {
  it("opens and closes on Ctrl+K and on Meta+K", async () => {
    render(createElement(Host));
    expect(screen.queryByRole("dialog")).toBeNull();
    press({ ctrlKey: true });
    expect(screen.getByRole("dialog")).toBeTruthy();
    press({ ctrlKey: true });
    await closed();
    press({ metaKey: true });
    expect(screen.getByRole("dialog")).toBeTruthy();
    press({ metaKey: true });
    await closed();
  });

  it("ignores K without a modifier and other modified keys", () => {
    render(createElement(Host));
    press({});
    press({ ctrlKey: true, shiftKey: true });
    press({ ctrlKey: true, altKey: true });
    fireEvent.keyDown(window, { key: "j", ctrlKey: true });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("prevents the browser default for the shortcut", () => {
    render(createElement(Host));
    const event = new KeyboardEvent("keydown", { key: "k", ctrlKey: true, cancelable: true });
    window.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(true);
  });

  it("can be turned off for a host that binds the key itself", () => {
    render(createElement(Host, { disableShortcut: true }));
    press({ ctrlKey: true });
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("works uncontrolled and reports changes", () => {
    const onOpenChange = vi.fn();
    render(createElement(ToolCommandPalette, { catalog, onSelectTool: () => {}, onOpenChange }));
    press({ ctrlKey: true });
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(onOpenChange).toHaveBeenLastCalledWith(true);
  });

  it("starts open with the seed text and clears it on close", async () => {
    render(
      createElement(ToolCommandPalette, {
        catalog,
        defaultOpen: true,
        initialSearch: "merge",
        onSelectTool: () => {},
      }),
    );
    expect(input().value).toBe("merge");
    fireEvent.keyDown(input(), { key: "Escape" });
    await closed();
    press({ ctrlKey: true });
    expect(input().value).toBe("merge");
  });
});

describe("accessibility", () => {
  it("is a named dialog with a labelled combobox controlling a listbox of options", () => {
    render(createElement(Host));
    press({ ctrlKey: true });
    const dialog = screen.getByRole("dialog", { name: "Tool command palette" });
    expect(within(dialog).getByRole("combobox").getAttribute("aria-label")).toBe("Search tools");
    const listbox = within(dialog).getByRole("listbox");
    expect(input().getAttribute("aria-controls")).toBe(listbox.id);
    expect(options().length).toBeGreaterThan(100);
    expect(input().getAttribute("aria-activedescendant")).toBe(
      options().find((option) => option.getAttribute("aria-selected") === "true")?.id,
    );
    expect(within(dialog).getAllByRole("group").length).toBeGreaterThan(5);
  });

  it("moves through options with the arrow keys", () => {
    render(createElement(Host));
    press({ ctrlKey: true });
    type("pdf.");
    const [first, second] = options();
    expect(first?.getAttribute("aria-selected")).toBe("true");
    fireEvent.keyDown(input(), { key: "ArrowDown" });
    expect(second?.getAttribute("aria-selected")).toBe("true");
    fireEvent.keyDown(input(), { key: "ArrowUp" });
    expect(first?.getAttribute("aria-selected")).toBe("true");
  });
});

describe("tools", () => {
  it("lists every tool grouped by category with runtime and tier badges", () => {
    render(createElement(Host));
    press({ ctrlKey: true });
    const rows = options().filter((option) => option.dataset.toolId);
    expect(new Set(rows.map((row) => row.dataset.toolId)).size).toBe(102);
    for (const row of rows) {
      expect(row.querySelector("[data-runtime]")?.textContent).toMatch(/Browser|Desktop|Cloud/);
      expect(row.querySelector("[data-tier]")?.textContent).toMatch(/Free|Pro/);
    }
    const headings = Array.from(
      document.querySelectorAll('[data-slot="command-group-heading"]'),
    ).map((heading) => heading.textContent);
    expect(headings).toEqual(
      expect.arrayContaining(["Organise & assemble", "Convert", "Security & signing"]),
    );
  });

  it("searches fuzzily and ranks the best match first", () => {
    render(createElement(Host));
    press({ ctrlKey: true });

    type("combine");
    expect(optionIds()[0]).toBe("pdf.merge");

    type("rota");
    expect(optionIds()[0]).toBe("pdf.rotate");

    type("mrg");
    expect(optionIds()).toContain("pdf.merge");

    type("password");
    expect(optionIds()).toEqual(expect.arrayContaining(["pdf.encrypt", "pdf.decrypt"]));
  });

  it("shows an empty state when nothing matches", () => {
    render(createElement(Host));
    press({ ctrlKey: true });
    type("zzzzqqqq");
    expect(screen.queryAllByRole("option")).toHaveLength(0);
    expect(screen.getByText(/No tool matches/)).toBeTruthy();
  });

  it("selects the active tool with Enter and closes", async () => {
    const onSelectTool = vi.fn();
    render(createElement(Host, { onSelectTool }));
    press({ ctrlKey: true });
    type("pdf.merge");
    fireEvent.keyDown(input(), { key: "Enter" });
    expect(onSelectTool).toHaveBeenCalledTimes(1);
    expect(onSelectTool).toHaveBeenCalledWith("pdf.merge");
    await closed();
  });

  it("selects a tool by click", () => {
    const onSelectTool = vi.fn();
    render(createElement(Host, { onSelectTool }));
    press({ ctrlKey: true });
    type("crop");
    const row = options().find((option) => option.dataset.toolId === "pdf.crop");
    if (!row) throw new Error("pdf.crop row missing");
    fireEvent.click(row);
    expect(onSelectTool).toHaveBeenCalledWith("pdf.crop");
  });

  it("shows favourites and recent tools first when the search is empty", () => {
    toggleFavorite("pdf.split");
    recordRecent("pdf.rotate");
    render(createElement(Host));
    press({ ctrlKey: true });
    const group = (name: RegExp) => screen.getByRole("group", { name });
    expect(
      within(group(/Favourites/))
        .getAllByRole("option")
        .map((row) => row.dataset.toolId),
    ).toEqual(["pdf.split"]);
    expect(
      within(group(/Recent tools/))
        .getAllByRole("option")
        .map((row) => row.dataset.toolId),
    ).toEqual(["pdf.rotate"]);
    // Searching replaces the groups with one ranked list.
    type("split");
    expect(screen.queryByRole("group", { name: /Favourites/ })).toBeNull();
  });

  it("ignores stored ids that are not in the catalog", () => {
    window.localStorage.setItem(FAVORITES_KEY, JSON.stringify(["pdf.gone", "pdf.merge"]));
    render(createElement(Host));
    press({ ctrlKey: true });
    const favourites = screen.getByRole("group", { name: /Favourites/ });
    expect(within(favourites).getAllByRole("option")).toHaveLength(1);
  });
});

describe("quick actions", () => {
  it("clears recent tools", async () => {
    recordRecent("pdf.rotate");
    render(createElement(Host));
    press({ ctrlKey: true });
    fireEvent.click(screen.getByText("Clear recent tools"));
    expect(window.localStorage.getItem(RECENTS_KEY)).toBeNull();
    await closed();
    press({ ctrlKey: true });
    expect(screen.queryByText("Clear recent tools")).toBeNull();
  });

  it("jumps to a category through the host", async () => {
    const onJumpToCategory = vi.fn();
    render(createElement(Host, { onJumpToCategory }));
    press({ ctrlKey: true });
    fireEvent.click(screen.getByText("Jump to Convert"));
    expect(onJumpToCategory).toHaveBeenCalledWith("convert");
    await closed();
  });

  it("only offers the jump when the host can handle it", () => {
    render(createElement(Host));
    press({ ctrlKey: true });
    expect(screen.queryByText("Jump to Convert")).toBeNull();
  });

  it("switches the theme and remembers it", () => {
    render(createElement(Host));
    press({ ctrlKey: true });
    fireEvent.click(screen.getByText("Switch theme"));
    expect(document.documentElement.classList.contains("dark")).toBe(true);
    expect(window.localStorage.getItem("ariadshift-theme")).toBe("dark");
  });

  it("lets the host own the theme switch", () => {
    const onSwitchTheme = vi.fn();
    render(createElement(Host, { onSwitchTheme }));
    press({ ctrlKey: true });
    fireEvent.click(screen.getByText("Switch theme"));
    expect(onSwitchTheme).toHaveBeenCalledTimes(1);
    expect(document.documentElement.classList.contains("dark")).toBe(false);
  });

  it("finds actions by search", () => {
    render(createElement(Host));
    press({ ctrlKey: true });
    type("dark");
    expect(screen.getByText("Switch theme")).toBeTruthy();
  });
});

describe("recipe from a plain sentence", () => {
  it("previews the steps as pills and loads them on Enter", async () => {
    const onRunRecipe = vi.fn();
    render(createElement(Host, { onRunRecipe }));
    press({ ctrlKey: true });
    type("rotate 90 degrees then compress");

    const preview = screen.getByTestId("recipe-preview");
    const steps = within(preview).getAllByTestId("recipe-step");
    expect(steps.map((step) => step.dataset.toolId)).toEqual(["pdf.rotate", "pdf.compress"]);
    expect(steps[0]?.textContent).toContain("Step 1");
    expect(steps[0]?.textContent).toContain("Rotate 90°");
    expect(steps[1]?.textContent).toContain("Step 2");
    expect(within(preview).getByRole("list", { name: "Recipe steps" })).toBeTruthy();
    // The recipe is the first option, so Enter loads it.
    expect(options()[0]).toBe(preview);
    expect(preview.getAttribute("aria-selected")).toBe("true");

    fireEvent.keyDown(input(), { key: "Enter" });
    expect(onRunRecipe).toHaveBeenCalledTimes(1);
    const [draft] = onRunRecipe.mock.calls[0] ?? [];
    expect(draft).toHaveLength(2);
    expect(draft[0]).toMatchObject({ toolId: "pdf.rotate", params: { angle: 90 } });
    expect(draft[0].tool.id).toBe("pdf.rotate");
    expect(draft[1]).toMatchObject({ toolId: "pdf.compress", params: {} });
    await closed();
  });

  it("loads a recipe by click", () => {
    const onRunRecipe = vi.fn();
    render(createElement(Host, { onRunRecipe }));
    press({ ctrlKey: true });
    type("merge and add watermark CONFIDENTIAL");
    fireEvent.click(screen.getByTestId("recipe-preview"));
    expect(onRunRecipe).toHaveBeenCalledTimes(1);
    expect(onRunRecipe.mock.calls[0]?.[0].map((step: { toolId: string }) => step.toolId)).toEqual([
      "pdf.merge",
      "pdf.watermark",
    ]);
    expect(onRunRecipe.mock.calls[0]?.[0][1].params).toEqual({ text: "CONFIDENTIAL" });
  });

  it("asks for review when a step leaves the browser, uses AI or cannot be undone", () => {
    render(createElement(Host, { onRunRecipe: () => {} }));
    press({ ctrlKey: true });
    type("ocr in vietnamese and summarize");
    const preview = screen.getByTestId("recipe-preview");
    expect(within(preview).getByText("Review first")).toBeTruthy();
    expect(preview.textContent).toContain("Review before running");

    type("merge and add watermark CONFIDENTIAL");
    expect(screen.queryByText("Review first")).toBeNull();
    expect(screen.getByTestId("recipe-preview").textContent).toContain("Press Enter");
  });

  it("keeps ordinary searches as searches", () => {
    render(createElement(Host, { onRunRecipe: () => {} }));
    press({ ctrlKey: true });
    type("compress");
    expect(screen.queryByTestId("recipe-preview")).toBeNull();
    expect(optionIds()[0]).toBe("pdf.compress");
  });

  it("hides the preview when the host cannot run recipes", () => {
    render(createElement(Host));
    press({ ctrlKey: true });
    type("rotate 90 degrees then compress");
    // No recipe preview, and a sentence is not a tool name, so plain search finds nothing.
    expect(screen.queryByTestId("recipe-preview")).toBeNull();
    expect(screen.queryAllByRole("option")).toHaveLength(0);
    type("rotate");
    expect(optionIds()[0]).toBe("pdf.rotate");
  });

  it("translates labels through the optional hook", () => {
    const translate = (key: string, fallback: string) =>
      key === "tools.palette.search" ? "Tìm công cụ" : fallback;
    render(createElement(Host, { translate }));
    press({ ctrlKey: true });
    expect(input().getAttribute("aria-label")).toBe("Tìm công cụ");
  });
});

describe("parseNaturalLanguageRecipe", () => {
  const parse = (text: string) => parseNaturalLanguageRecipe(text, catalog);
  const summary = (text: string) =>
    parse(text)?.steps.map((step) => [step.toolId, step.params] as const);

  it("parses the documented sentences", () => {
    expect(summary("rotate 90 degrees then compress")).toEqual([
      ["pdf.rotate", { angle: 90 }],
      ["pdf.compress", {}],
    ]);
    expect(summary("merge and add watermark CONFIDENTIAL")).toEqual([
      ["pdf.merge", {}],
      ["pdf.watermark", { text: "CONFIDENTIAL" }],
    ]);
    // The OCR tool takes `languages` (an array of Tesseract codes), not a single `lang`.
    expect(summary("ocr in vietnamese and summarize")).toEqual([
      ["pdf.ocr", { languages: ["vie"] }],
      ["ai.summarize", {}],
    ]);
  });

  it("returns the full step shape", () => {
    const draft = parse("rotate 90 degrees then compress");
    expect(draft?.steps[0]).toEqual({
      toolId: "pdf.rotate",
      tool: expect.objectContaining({ id: "pdf.rotate" }),
      params: { angle: 90 },
      label: "Rotate 90°",
    });
    expect(draft?.requiresConfirmation).toBe(true);
    expect(parse("merge and add watermark CONFIDENTIAL")?.requiresConfirmation).toBe(false);
  });

  it("treats a plain single-tool search as a search", () => {
    expect(parse("compress")).toBeNull();
    expect(parse("page numbers")).toBeNull();
    expect(parse("extract pages")).toBeNull();
    expect(parse("")).toBeNull();
    expect(parse("   ")).toBeNull();
    expect(parseNaturalLanguageRecipe("rotate then compress", [])).toBeNull();
  });

  it("returns null when any part of the sentence names no tool", () => {
    expect(parse("compress and zzzzqqqq")).toBeNull();
    expect(parse("zzzzqqqq then compress")).toBeNull();
  });

  it("returns one step when a single tool has parameters", () => {
    expect(summary("rotate 90")).toEqual([["pdf.rotate", { angle: 90 }]]);
    expect(summary("rotate pages 1-3 counterclockwise")).toEqual([
      ["pdf.rotate", { angle: -90, pages: "1-3" }],
    ]);
    expect(summary("delete pages 2, 4-6")).toEqual([["pdf.delete_pages", { pages: "2, 4-6" }]]);
  });

  it("does not read a page range as an angle", () => {
    expect(summary("rotate pages 90-100 180")).toEqual([
      ["pdf.rotate", { angle: 180, pages: "90-100" }],
    ]);
  });

  it("splits on commas, arrows and other connectors", () => {
    const ids = (text: string) => parse(text)?.steps.map((step) => step.toolId);
    expect(ids("merge, compress, summarize")).toEqual([
      "pdf.merge",
      "pdf.compress",
      "ai.summarize",
    ]);
    expect(ids("merge -> compress")).toEqual(["pdf.merge", "pdf.compress"]);
    expect(ids("merge, then compress and then encrypt")).toEqual([
      "pdf.merge",
      "pdf.compress",
      "pdf.encrypt",
    ]);
    expect(ids("Merge AND Compress")).toEqual(["pdf.merge", "pdf.compress"]);
  });

  it("keeps quoted text whole and keeps its casing", () => {
    expect(summary('merge then watermark "Top and Secret"')).toEqual([
      ["pdf.merge", {}],
      ["pdf.watermark", { text: "Top and Secret" }],
    ]);
    expect(summary("watermark Draft v2 then compress")?.[0]).toEqual([
      "pdf.watermark",
      { text: "Draft v2" },
    ]);
  });

  it("keeps black and white together", () => {
    expect(summary("convert to black and white then compress")?.map(([id]) => id)).toEqual([
      "pdf.grayscale",
      "pdf.compress",
    ]);
  });

  it("merges parameters of a repeated tool instead of duplicating the step", () => {
    expect(summary("rotate pages 1-2 and rotate 90")).toEqual([
      ["pdf.rotate", { pages: "1-2", angle: 90 }],
    ]);
  });

  it("only keeps parameters the tool's schema declares", () => {
    const stripped = catalog.map((tool) =>
      tool.id === "pdf.watermark" ? { ...tool, params_schema: { properties: {} } } : tool,
    );
    const draft = parseNaturalLanguageRecipe("merge and add watermark CONFIDENTIAL", stripped);
    expect(draft?.steps[1]?.params).toEqual({});
  });

  it("recognises languages and falls back to search for tools without a phrase", () => {
    expect(summary("ocr in simplified chinese and english")?.[0]).toEqual([
      "pdf.ocr",
      { languages: ["chi_sim", "eng"] },
    ]);
    expect(parse("merge then page info")?.steps.map((step) => step.toolId)).toEqual([
      "pdf.merge",
      "pdf.info",
    ]);
  });

  it("asks for confirmation only when a step leaves the browser, uses AI or cannot be undone", () => {
    expect(parse("merge then sign")?.requiresConfirmation).toBe(false);
    expect(parse("merge then encrypt")?.requiresConfirmation).toBe(true);
    expect(parse("merge then summarize")?.requiresConfirmation).toBe(true);
    expect(parse("merge then redact")?.requiresConfirmation).toBe(true);
  });
});
