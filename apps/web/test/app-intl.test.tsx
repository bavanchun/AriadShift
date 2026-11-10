// @vitest-environment jsdom

import { readFileSync } from "node:fs";
import path from "node:path";
import { cleanup, render, renderHook, screen } from "@testing-library/react";
import { NextIntlClientProvider } from "next-intl";
import type { ReactNode } from "react";
import ToolsHost from "@/components/app/tools-host";
import { catalogueKey, useToolsTranslate } from "@/components/tools/use-tools-translate";
import { DEFAULT_WORKSPACE_COPY } from "@/components/workspace/copy";
import { useWorkspaceIntl } from "@/components/workspace/use-workspace-copy";
import { toolCatalog } from "@/converter/tool-catalog";
import { loadMessages, type Messages, scopeMessages } from "@/i18n/messages";
import { TOOL_CATEGORIES } from "@/lib/tools";
import { DEFAULT_VIEWER_LABELS } from "@/viewer/types";

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

afterEach(cleanup);

const NAMESPACES = ["common", "web.app", "web.tools", "web.workspace"];
const wrapper =
  (locale: string) =>
  ({ children }: { children: ReactNode }) => (
    <NextIntlClientProvider
      locale={locale}
      messages={scopeMessages(loadMessages(locale), NAMESPACES)}
      timeZone="UTC"
    >
      {children}
    </NextIntlClientProvider>
  );

function lookup(messages: Messages, dotted: string): unknown {
  let node: string | Messages | undefined = messages;
  for (const part of dotted.split(".")) {
    node = typeof node === "object" ? node[part] : undefined;
  }
  return node;
}

describe("catalogue keys of the tool hub", () => {
  it("turns a component key into a catalogue key", () => {
    expect(catalogueKey("tools.hub.openPalette")).toBe("hub.open_palette");
    expect(catalogueKey("tools.title.pdf.extract_pages")).toBe("title.pdf.extract_pages");
  });

  for (const locale of ["en", "vi"]) {
    it(`defines every key the hub, card and palette use, in ${locale}`, () => {
      const messages = loadMessages(locale);
      const source = ["hub", "card", "palette"]
        .map((file) =>
          readFileSync(
            path.join(import.meta.dirname, `../src/components/tools/${file}.tsx`),
            "utf8",
          ),
        )
        .join("\n");
      const literal = [...source.matchAll(/\bt\(\s*"(tools\.[A-Za-z_.]+)"/g)].map((m) => m[1]);
      expect(literal.length).toBeGreaterThan(30);
      const missing = literal.filter(
        (key) => typeof lookup(messages, `web.tools.${catalogueKey(key as string)}`) !== "string",
      );
      expect(missing).toEqual([]);
      for (const category of TOOL_CATEGORIES)
        expect(typeof lookup(messages, `web.tools.category.${category}`)).toBe("string");
      for (const runtime of ["browser", "desktop", "cloud"])
        expect(typeof lookup(messages, `web.tools.runtime.${runtime}`)).toBe("string");
      for (const tier of ["free", "pro"])
        expect(typeof lookup(messages, `web.tools.tier.${tier}`)).toBe("string");
    });

    it(`gives every registered tool a title and a summary in ${locale}`, () => {
      const messages = loadMessages(locale);
      const missing = toolCatalog().flatMap((tool) =>
        ["title", "summary"]
          .filter((part) => typeof lookup(messages, `web.tools.${part}.${tool.id}`) !== "string")
          .map((part) => `${part}.${tool.id}`),
      );
      expect(missing).toEqual([]);
    });
  }
});

describe("useToolsTranslate", () => {
  it("translates known keys and keeps the fallback for unknown ones", () => {
    const { result } = renderHook(() => useToolsTranslate(), { wrapper: wrapper("vi") });
    expect(result.current("tools.hub.label", "Tools")).toBe("Công cụ");
    expect(result.current("tools.hub.count", "7 tools", { count: 7 })).toBe("7 công cụ");
    expect(result.current("tools.title.pdf.future_tool", "Future tool")).toBe("Future tool");
  });
});

describe("workspace copy from the catalogue", () => {
  it("reads the same in English as the built-in defaults", () => {
    const { result } = renderHook(() => useWorkspaceIntl(), { wrapper: wrapper("en") });
    const { copy, viewerLabels } = result.current;
    const defaults = DEFAULT_WORKSPACE_COPY;
    expect(copy.fileTray.privacy).toBe(defaults.fileTray.privacy);
    expect(copy.fileTray.pages(1)).toBe(defaults.fileTray.pages(1));
    expect(copy.fileTray.pages(3)).toBe(defaults.fileTray.pages(3));
    expect(copy.fileTray.remove("a.pdf")).toBe(defaults.fileTray.remove("a.pdf"));
    expect(copy.fileTray.storage("opfs")).toBe(defaults.fileTray.storage("opfs"));
    expect(copy.insights.dpi(150, 300)).toBe(defaults.insights.dpi(150, 300));
    expect(copy.insights.dpi(300, 300)).toBe(defaults.insights.dpi(300, 300));
    expect(copy.insights.dpi(null, null)).toBe(defaults.insights.dpi(null, null));
    expect(copy.insights.lowDpi([1, 2, 3])).toBe(defaults.insights.lowDpi([1, 2, 3]));
    expect(copy.insights.lowDpi([1, 2, 3, 4, 5, 6, 7])).toBe(
      defaults.insights.lowDpi([1, 2, 3, 4, 5, 6, 7]),
    );
    expect(copy.insights.scanned(1)).toBe(defaults.insights.scanned(1));
    expect(copy.pageGrid.pending(2)).toBe(defaults.pageGrid.pending(2));
    expect(copy.pageGrid.moved(3, 1)).toBe(defaults.pageGrid.moved(3, 1));
    expect(copy.inspector.failed("boom")).toBe(defaults.inspector.failed("boom"));
    expect(copy.inspector.runtime).toEqual(defaults.inspector.runtime);
    expect(copy.result.change(-12)).toBe(defaults.result.change(-12));
    expect(copy.result.change(0)).toBe(defaults.result.change(0));
    expect(copy.result.change(5)).toBe(defaults.result.change(5));
    expect(copy.result.seconds(450)).toBe(defaults.result.seconds(450));
    expect(copy.result.seconds(2500)).toBe(defaults.result.seconds(2500));
    expect(copy.handoff).toEqual(defaults.handoff);
    expect(copy.thread).toEqual(defaults.thread);
    expect(copy.workspace).toEqual(defaults.workspace);
    expect(viewerLabels.matches(2, 14)).toBe(DEFAULT_VIEWER_LABELS.matches(2, 14));
    expect(viewerLabels.pageOf(120)).toBe(DEFAULT_VIEWER_LABELS.pageOf(120));
    expect(viewerLabels.searching(40)).toBe(DEFAULT_VIEWER_LABELS.searching(40));
    expect(viewerLabels.documentError).toBe(DEFAULT_VIEWER_LABELS.documentError);
  });

  it("names tools from the catalogue, in Vietnamese", () => {
    const { result } = renderHook(() => useWorkspaceIntl(), { wrapper: wrapper("vi") });
    expect(result.current.copy.tools.title("pdf.delete_pages")).toBe("Xóa trang");
    expect(result.current.copy.tools.title("pdf.future_tool")).toBe("Future tool");
    expect(result.current.copy.fileTray.pages(4)).toBe("4 trang");
    expect(result.current.copy.result.seconds(2500)).toBe("2,5 giây");
  });
});

describe("ToolsHost", () => {
  // Rendering all 102 cards in jsdom is slow when the suite runs in parallel.
  it("renders the hub in Vietnamese", { timeout: 30_000 }, () => {
    render(<ToolsHost />, { wrapper: wrapper("vi") });
    expect(screen.getByRole("region", { name: "Công cụ" })).toBeTruthy();
    expect(screen.getByPlaceholderText("Tìm công cụ…")).toBeTruthy();
    expect(screen.getByTestId("result-count").textContent).toBe(`${toolCatalog().length} công cụ`);
    expect(screen.getByTestId("group-organize").getAttribute("aria-label")).toBe(
      "Sắp xếp & lắp ghép",
    );
  });
});
