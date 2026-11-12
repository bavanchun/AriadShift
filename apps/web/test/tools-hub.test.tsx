// @vitest-environment jsdom

import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { createElement } from "react";
import { ToolHub } from "@/components/tools/hub";
import { toolCatalog } from "@/converter/tool-catalog";
import {
  clearRecents,
  describeTools,
  FAVORITES_KEY,
  primaryRuntime,
  RECENTS_KEY,
  RECENTS_LIMIT,
  readFavorites,
  readRecents,
  recordRecent,
  searchTools,
  TOOL_CATEGORIES,
  toggleFavorite,
  toolCategory,
  toolRuntimes,
  toolTier,
  toolTitle,
} from "@/lib/tools";
import registry from "../../../schemas/tools.v1.json";

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

// Rendering all 102 cards in jsdom is slow; each test renders the whole grid at least once.
vi.setConfig({ testTimeout: 30_000 });

const catalog = toolCatalog();
const entries = describeTools(catalog);
const ids = (list: Array<{ id: string }>) => list.map((entry) => entry.id);
const cards = () => Array.from(document.querySelectorAll<HTMLElement>('[data-slot="tool-card"]'));
const cardFor = (id: string) => {
  const card = cards().find((element) => element.dataset.toolId === id);
  if (!card) throw new Error(`No card for ${id}`);
  return card;
};
const searchBox = () => screen.getByRole("searchbox");

beforeEach(() => {
  window.localStorage.clear();
});
afterEach(() => {
  cleanup();
  window.localStorage.clear();
  vi.restoreAllMocks();
});

describe("tool model", () => {
  it("covers every tool in the registry with a valid category, runtime badge and tier badge", () => {
    expect(registry.tools).toHaveLength(102);
    expect(ids(catalog).sort()).toEqual(registry.tools.map((tool) => tool.id).sort());
    for (const entry of entries) {
      expect(TOOL_CATEGORIES).toContain(entry.category);
      expect(["browser", "desktop", "cloud"]).toContain(entry.runtime);
      expect(["free", "pro"]).toContain(entry.tier);
      expect(entry.runtimes.length).toBeGreaterThan(0);
      expect(entry.title).not.toBe("");
    }
  });

  it("uses every category at least once", () => {
    const used = new Set(entries.map((entry) => entry.category));
    for (const category of TOOL_CATEGORIES) {
      expect(used, category).toContain(category);
    }
  });

  it("places tools in the categories the parity matrix describes", () => {
    const category = (id: string) => entries.find((entry) => entry.id === id)?.category;
    expect(category("pdf.merge")).toBe("organize");
    expect(category("pdf.booklet")).toBe("organize");
    expect(category("pdf.compress")).toBe("compress");
    expect(category("pdf.sanitize")).toBe("compress");
    expect(category("pdf.watermark")).toBe("edit");
    expect(category("pdf.forms_fill")).toBe("forms_annotations");
    expect(category("pdf.annotations")).toBe("forms_annotations");
    expect(category("pdf.encrypt")).toBe("security");
    expect(category("pdf.sign_verify")).toBe("security");
    expect(category("pdf.ocr")).toBe("ocr_scan");
    expect(category("pdf.remove_blank")).toBe("ocr_scan");
    expect(category("pdf.info")).toBe("metadata_inspect");
    expect(category("pdf.compare")).toBe("metadata_inspect");
    expect(category("pdf.validate_pdfa")).toBe("metadata_inspect");
    expect(category("pdf.to_images")).toBe("convert");
    expect(category("image.to_pdf")).toBe("convert");
    expect(category("ai.summarize")).toBe("automation_ai");
    expect(category("pdf.watermark_remove_raster")).toBe("automation_ai");
  });

  it("puts a tool added later in a category without a code change", () => {
    expect(toolCategory({ id: "pdf.denoise_scan", group: "future" })).toBe("ocr_scan");
    expect(toolCategory({ id: "pdf.add_certificate", group: "future" })).toBe("security");
    expect(toolCategory({ id: "ai.classify", group: "future" })).toBe("automation_ai");
    expect(toolCategory({ id: "pdf.mystery", group: "future" })).toBe("organize");
    expect(toolCategory({ id: "pdf.newthing", group: "convert" })).toBe("convert");
  });

  it("derives the runtime badge from the registry runtimes", () => {
    expect(primaryRuntime({ runtimes: ["wasm", "local", "cloud"] })).toBe("browser");
    expect(primaryRuntime({ runtimes: ["wasm", "local"] })).toBe("browser");
    expect(primaryRuntime({ runtimes: ["local", "cloud"] })).toBe("cloud");
    expect(primaryRuntime({ runtimes: ["local"] })).toBe("desktop");
    expect(toolRuntimes({ runtimes: ["cloud", "wasm", "local"] })).toEqual([
      "browser",
      "desktop",
      "cloud",
    ]);
    expect(toolTier({ tier: "pro" })).toBe("pro");
    expect(toolTier({ tier: "free" })).toBe("free");
    const pro = entries.filter((entry) => entry.tier === "pro").map((entry) => entry.id);
    expect(pro.sort()).toEqual(["edit.apply", "edit.page_objects", "pdf.sign_token"]);
  });

  it("writes readable titles", () => {
    expect(toolTitle({ id: "pdf.extract_pages" })).toBe("Extract pages");
    expect(toolTitle({ id: "pdf.nup" })).toBe("N-up");
    expect(toolTitle({ id: "image.to_pdf" })).toBe("Image to PDF");
    expect(toolTitle({ id: "ai.summarize" })).toBe("AI summarize");
    expect(toolTitle({ id: "edit.page_objects" })).toBe("Page objects");
  });

  it("searches by title, id, synonym, summary and category, ranked and typo tolerant", () => {
    expect(ids(searchTools(entries, "combine"))[0]).toBe("pdf.merge");
    expect(ids(searchTools(entries, "pdf.merge"))[0]).toBe("pdf.merge");
    expect(ids(searchTools(entries, "password"))).toEqual(
      expect.arrayContaining(["pdf.encrypt", "pdf.decrypt"]),
    );
    expect(ids(searchTools(entries, "mrg"))).toContain("pdf.merge");
    expect(ids(searchTools(entries, "rotate"))[0]).toBe("pdf.rotate");
    expect(searchTools(entries, "zzzzqqqq")).toEqual([]);
    expect(searchTools(entries, "  ")).toHaveLength(102);
    // Every word must match: no tool is both "merge" and "ocr".
    expect(searchTools(entries, "merge ocr")).toEqual([]);
  });

  it("filters by category, runtime and tier", () => {
    const security = searchTools(entries, "", { category: "security" });
    expect(security.length).toBeGreaterThan(5);
    expect(security.every((entry) => entry.category === "security")).toBe(true);
    const desktop = searchTools(entries, "", { runtime: "desktop" });
    expect(desktop.length).toBeGreaterThan(0);
    expect(desktop.every((entry) => entry.runtime === "desktop")).toBe(true);
    expect(searchTools(entries, "", { tier: "pro" })).toHaveLength(3);
    const both = searchTools(entries, "", { category: "ocr_scan", runtime: "cloud" });
    expect(both.every((entry) => entry.category === "ocr_scan" && entry.runtime === "cloud")).toBe(
      true,
    );
  });
});

describe("favourites and recents storage", () => {
  it("persists favourites under the versioned key and toggles them", () => {
    expect(readFavorites()).toEqual([]);
    expect(toggleFavorite("pdf.merge")).toBe(true);
    expect(toggleFavorite("pdf.split")).toBe(true);
    expect(JSON.parse(window.localStorage.getItem(FAVORITES_KEY) ?? "[]")).toEqual([
      "pdf.split",
      "pdf.merge",
    ]);
    expect(toggleFavorite("pdf.merge")).toBe(false);
    expect(readFavorites()).toEqual(["pdf.split"]);
  });

  it("records recents newest first without duplicates, bounded to 12", () => {
    for (let index = 0; index < 20; index += 1) {
      recordRecent(`pdf.tool_${index}`);
    }
    expect(RECENTS_LIMIT).toBe(12);
    expect(readRecents()).toHaveLength(12);
    expect(readRecents()[0]).toBe("pdf.tool_19");
    recordRecent("pdf.tool_10");
    expect(readRecents()[0]).toBe("pdf.tool_10");
    expect(new Set(readRecents()).size).toBe(readRecents().length);
    expect(JSON.parse(window.localStorage.getItem(RECENTS_KEY) ?? "[]")).toHaveLength(12);
    clearRecents();
    expect(readRecents()).toEqual([]);
    expect(window.localStorage.getItem(RECENTS_KEY)).toBeNull();
  });

  it("ignores corrupt storage", () => {
    window.localStorage.setItem(FAVORITES_KEY, "{not json");
    expect(readFavorites()).toEqual([]);
    window.localStorage.setItem(FAVORITES_KEY, JSON.stringify(["pdf.merge", 4, "pdf.merge"]));
    expect(readFavorites()).toEqual(["pdf.merge"]);
  });

  it("keeps working in memory when storage refuses writes", async () => {
    vi.resetModules();
    const fresh = await import("@/lib/tools");
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("quota", "QuotaExceededError");
    });
    expect(() => fresh.toggleFavorite("pdf.merge")).not.toThrow();
    expect(fresh.readFavorites()).toEqual(["pdf.merge"]);
    expect(() => fresh.recordRecent("pdf.split")).not.toThrow();
    expect(fresh.readRecents()).toEqual(["pdf.split"]);
  });
});

describe("<ToolHub />", () => {
  it("shows every registered tool with category, runtime and tier badges", () => {
    render(createElement(ToolHub));
    expect(cards()).toHaveLength(102);
    for (const card of cards()) {
      expect(TOOL_CATEGORIES).toContain(card.dataset.category);
      expect(["browser", "desktop", "cloud"]).toContain(card.dataset.runtime);
      expect(within(card).getByTestId("runtime-badge").textContent).toMatch(
        /Browser|Desktop|Cloud/,
      );
      expect(within(card).getByTestId("tier-badge").textContent).toMatch(/Free|Pro/);
      // The open action and the favourite star.
      expect(within(card).getAllByRole("button")).toHaveLength(2);
    }
    expect(within(cardFor("pdf.sign_token")).getByTestId("tier-badge").textContent).toBe("Pro");
    expect(within(cardFor("pdf.merge")).getByTestId("runtime-badge").textContent).toContain(
      "Browser",
    );
    expect(within(cardFor("pdf.encrypt")).getByTestId("runtime-badge").textContent).toContain(
      "Desktop",
    );
    expect(within(cardFor("pdf.compress")).getByTestId("runtime-badge").textContent).toContain(
      "Cloud",
    );
    // One group per category, in the canonical order.
    const groups = Array.from(document.querySelectorAll('[data-testid^="group-"]')).map((group) =>
      group.getAttribute("data-testid"),
    );
    expect(groups).toEqual(TOOL_CATEGORIES.map((category) => `group-${category}`));
  });

  it("shows the on-device trust pill", () => {
    render(createElement(ToolHub));
    expect(screen.getByTestId("zero-upload-badge").textContent).toBe(
      "On this device · 0 bytes uploaded",
    );
  });

  it("filters by search text and offers a reset when nothing matches", () => {
    render(createElement(ToolHub));
    fireEvent.change(searchBox(), { target: { value: "combine" } });
    expect(cards().map((card) => card.dataset.toolId)).toContain("pdf.merge");
    expect(cards().length).toBeLessThan(10);
    expect(screen.getByTestId("result-count").textContent).toBe(`${cards().length} tools`);

    fireEvent.change(searchBox(), { target: { value: "zzzzqqqq" } });
    expect(cards()).toHaveLength(0);
    const empty = screen.getByTestId("empty-state");
    expect(empty.textContent).toContain("zzzzqqqq");
    expect(empty.textContent).toContain("rotate 90 degrees then compress");

    fireEvent.click(within(empty).getByRole("button", { name: /reset/i }));
    expect(cards()).toHaveLength(102);
    expect((searchBox() as HTMLInputElement).value).toBe("");
  });

  it("filters by category and returns to all", () => {
    render(createElement(ToolHub));
    fireEvent.click(screen.getByTestId("category-security"));
    expect(cards().length).toBeGreaterThan(5);
    expect(cards().every((card) => card.dataset.category === "security")).toBe(true);
    expect(screen.queryByTestId("group-organize")).toBeNull();
    fireEvent.click(screen.getByTestId("category-all"));
    expect(cards()).toHaveLength(102);
  });

  it("filters by runtime", () => {
    render(createElement(ToolHub));
    const expected = (runtime: string) => entries.filter((entry) => entry.runtime === runtime);
    for (const runtime of ["browser", "desktop", "cloud"] as const) {
      fireEvent.click(screen.getByTestId(`runtime-${runtime}`));
      expect(cards()).toHaveLength(expected(runtime).length);
      expect(cards().every((card) => card.dataset.runtime === runtime)).toBe(true);
    }
    fireEvent.click(screen.getByTestId("runtime-all"));
    expect(cards()).toHaveLength(102);
  });

  it("combines search with filters", () => {
    render(createElement(ToolHub));
    fireEvent.click(screen.getByTestId("runtime-browser"));
    fireEvent.change(searchBox(), { target: { value: "pages" } });
    expect(cards().length).toBeGreaterThan(0);
    expect(cards().every((card) => card.dataset.runtime === "browser")).toBe(true);
  });

  it("toggles a favourite without selecting the tool, and keeps it across a reload", () => {
    const onSelectTool = vi.fn();
    const { unmount } = render(createElement(ToolHub, { onSelectTool }));
    expect(screen.queryByTestId("favorites-strip")).toBeNull();

    const star = within(cardFor("pdf.merge")).getByTestId("favorite-toggle");
    expect(star.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(star);

    expect(onSelectTool).not.toHaveBeenCalled();
    expect(JSON.parse(window.localStorage.getItem(FAVORITES_KEY) ?? "[]")).toEqual(["pdf.merge"]);
    const strip = screen.getByTestId("favorites-strip");
    expect(within(strip).getAllByRole("heading", { level: 3 })[0]?.textContent).toBe("Merge");
    expect(
      within(cardFor("pdf.merge")).getByTestId("favorite-toggle").getAttribute("aria-pressed"),
    ).toBe("true");
    expect(readRecents()).toEqual([]);

    unmount();
    render(createElement(ToolHub));
    expect(within(screen.getByTestId("favorites-strip")).getAllByRole("article")).toHaveLength(1);

    fireEvent.click(within(screen.getByTestId("favorites-strip")).getByTestId("favorite-toggle"));
    expect(screen.queryByTestId("favorites-strip")).toBeNull();
    expect(readFavorites()).toEqual([]);
  });

  it("is operable from the keyboard: the star is a focusable pressed-state button", () => {
    render(createElement(ToolHub));
    const star = within(cardFor("pdf.split")).getByTestId("favorite-toggle");
    expect(star.tagName).toBe("BUTTON");
    expect(star.getAttribute("aria-label")).toContain("Split");
    star.focus();
    expect(document.activeElement).toBe(star);
  });

  it("records the selected tool as a recent and shows it in the recent strip", () => {
    const onSelectTool = vi.fn();
    render(createElement(ToolHub, { onSelectTool }));
    expect(screen.queryByTestId("recents-strip")).toBeNull();
    fireEvent.click(within(cardFor("pdf.rotate")).getByRole("button", { name: "Rotate" }));

    expect(onSelectTool).toHaveBeenCalledTimes(1);
    expect(onSelectTool).toHaveBeenCalledWith("pdf.rotate");
    expect(JSON.parse(window.localStorage.getItem(RECENTS_KEY) ?? "[]")).toEqual(["pdf.rotate"]);
    const strip = screen.getByTestId("recents-strip");
    expect(within(strip).getAllByRole("article")).toHaveLength(1);

    // Strips give way to results while searching.
    fireEvent.change(searchBox(), { target: { value: "split" } });
    expect(screen.queryByTestId("recents-strip")).toBeNull();
  });

  it("marks the controlled selection and follows it", () => {
    const { rerender } = render(createElement(ToolHub, { selectedToolId: "pdf.crop" }));
    expect(cardFor("pdf.crop").dataset.selected).toBe("true");
    expect(cardFor("pdf.merge").dataset.selected).toBeUndefined();
    rerender(createElement(ToolHub, { selectedToolId: "pdf.merge" }));
    expect(cardFor("pdf.merge").dataset.selected).toBe("true");
    expect(cardFor("pdf.crop").dataset.selected).toBeUndefined();
  });

  it("tracks the selection itself when uncontrolled", () => {
    render(createElement(ToolHub));
    fireEvent.click(within(cardFor("pdf.crop")).getByRole("button", { name: "Crop" }));
    expect(cardFor("pdf.crop").dataset.selected).toBe("true");
  });

  it("merges runtime-reported tools into the grid", () => {
    const runtimeTools = [
      {
        id: "pdf.brand_new",
        version: "1",
        group: "organize",
        summary: "A tool the registry does not know yet.",
        inputs: { arity: "one" },
        input_formats: ["pdf"],
        output_format: "pdf",
        params_schema: {},
        tier: "free",
        runtimes: ["wasm"],
        engine: "ariad-pdf",
        secret_params: [],
      },
    ] as unknown as Parameters<typeof toolCatalog>[0];
    render(createElement(ToolHub, { runtimeTools }));
    expect(cards()).toHaveLength(103);
    expect(cardFor("pdf.brand_new").dataset.runtime).toBe("browser");
  });

  it("opens the command palette from the ⌘K button and from the shortcut", async () => {
    const onOpenPalette = vi.fn();
    render(createElement(ToolHub, { onOpenPalette }));
    expect(screen.queryByRole("dialog")).toBeNull();
    fireEvent.click(screen.getByTestId("palette-trigger"));
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(onOpenPalette).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(window, { key: "k", metaKey: true });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(onOpenPalette).toHaveBeenCalledTimes(2);
  });

  it("jumps to a category from the palette and selects tools through the hub", async () => {
    const onSelectTool = vi.fn();
    render(createElement(ToolHub, { onSelectTool }));
    fireEvent.click(screen.getByTestId("palette-trigger"));
    const dialog = screen.getByRole("dialog");
    fireEvent.click(within(dialog).getByText("Jump to Convert"));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(cards().every((card) => card.dataset.category === "convert")).toBe(true);

    fireEvent.click(screen.getByTestId("palette-trigger"));
    const input = screen.getByRole("combobox");
    fireEvent.change(input, { target: { value: "pdf.merge" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSelectTool).toHaveBeenCalledWith("pdf.merge");
    expect(readRecents()).toEqual(["pdf.merge"]);
  });

  it("offers an empty search to the palette as a starting point", () => {
    render(createElement(ToolHub));
    fireEvent.change(searchBox(), { target: { value: "rotate 90 then compress" } });
    // A sentence is not a tool name, so the grid is empty and the palette can take it from here.
    fireEvent.click(
      within(screen.getByTestId("empty-state")).getByRole("button", { name: /palette/i }),
    );
    expect((screen.getByRole("combobox") as HTMLInputElement).value).toBe(
      "rotate 90 then compress",
    );
  });

  it("translates labels through the optional hook and falls back to English", () => {
    const translate = (key: string, fallback: string) =>
      key === "tools.hub.zeroUpload" ? "Trên thiết bị này · 0 byte tải lên" : fallback;
    render(createElement(ToolHub, { translate }));
    expect(screen.getByTestId("zero-upload-badge").textContent).toBe(
      "Trên thiết bị này · 0 byte tải lên",
    );
    expect(cards()).toHaveLength(102);
  });
});
