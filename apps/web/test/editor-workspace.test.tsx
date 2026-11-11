// @vitest-environment jsdom

import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import RecipesHost from "@/components/app/recipes-host";
import { DocumentWorkspace } from "@/components/workspace/document-workspace";
import {
  annotationsFromList,
  fieldsFromList,
  initialPointFor,
  locationFromSelection,
  pagesFromInfo,
} from "@/components/workspace/editor-data";
import type { ConvertedFile } from "@/converter/protocol";
import type { IrDocument } from "@/editor/document/mapping";
import {
  mapEnginePageObjects,
  PageEditSession,
  type PageObject,
  pageObjectsFromIr,
  resolveFontForEdit,
  type TextRunObject,
} from "@/editor/page";
import mixedDocument from "../../../fixtures/editor/mixed-document.ir.json";
import { fakeBackend } from "./viewer-fixtures";
import { fakeClient, pdfFile } from "./workspace-fixtures";

afterEach(cleanup);

const ir = mixedDocument as unknown as IrDocument;

describe("mapEnginePageObjects", () => {
  const report = {
    schema: "ariad-pdf-page-objects/1",
    page_count: 1,
    pages: [
      {
        page: 1,
        width_pt: 595.28,
        height_pt: 841.89,
        rotation_deg: 0,
        objects: [
          {
            id: "p1_obj0",
            page: 1,
            index: 0,
            kind: "text",
            bbox: [62, 700, 300, 716],
            text: "Khu vườn đọc sách",
            font_name: "ABCDEF+Helvetica",
            font_size: 15.4,
            fill_color: "#112233FF",
            matrix: [1, 0, 0, 1, 0, 0],
          },
          {
            id: "p1_obj1",
            page: 1,
            index: 1,
            kind: "text",
            bbox: [62, 600, 300, 616],
            text: "Plain",
            font_name: "Helvetica",
            matrix: [1, 0, 0, 1, 0, 0],
          },
          {
            id: "p1_obj2",
            page: 1,
            index: 2,
            kind: "image",
            bbox: [0, 0, 10, 10],
            width_px: 20,
            height_px: 30,
            matrix: [1, 0, 0, 1, 0, 0],
          },
          {
            id: "p1_obj3",
            page: 1,
            index: 3,
            kind: "path",
            bbox: [0, 0, 5, 5],
            stroke_color: "#FF0000",
            matrix: [1, 0, 0, 1, 0, 0],
          },
        ],
      },
    ],
  };

  it("maps text, image and path objects and the page size", () => {
    const model = mapEnginePageObjects(report);
    expect(model.source).toBe("engine");
    expect(model.pages).toEqual([{ page: 1, widthPt: 595.28, heightPt: 841.89 }]);
    expect(model.objects.map((object) => object.kind)).toEqual(["text", "text", "image", "path"]);
    const subset = model.objects[0] as TextRunObject;
    expect(subset).toMatchObject({
      id: "p1_obj0",
      fontName: "ABCDEF+Helvetica",
      fontSize: 15.4,
      color: "#112233",
      isSubset: true,
    });
    expect(model.objects[2]).toMatchObject({ width: 20, height: 30 });
    expect(model.objects[3]).toMatchObject({ strokeColor: "#FF0000" });
  });

  it("gives a subset font the glyphs its runs already show, and a full font none", () => {
    const [subset, full] = mapEnginePageObjects(report).objects as TextRunObject[];
    expect(subset?.embeddedGlyphs).toContain("ờ");
    expect(subset?.embeddedGlyphs).not.toContain("ễ");
    expect(full?.isSubset).toBe(false);
    expect(full?.embeddedGlyphs).toBeUndefined();
  });

  it("warns about Vietnamese the subset never drew, with Noto Sans as the fallback", () => {
    const [subset] = mapEnginePageObjects(report).objects as TextRunObject[];
    const resolution = resolveFontForEdit(subset as TextRunObject, "Tiếng Việt: Nguyễn Văn A");
    expect(resolution.substituted).toBe(true);
    expect(resolution.tier).toBe("noto-vietnamese");
    expect(resolution.resolvedLabel).toBe("Noto Sans (Vietnamese coverage)");
    expect(resolution.originalLabel).toBe("Subset Helvetica");
    expect(resolution.missingChars).toContain("ễ");
  });

  it("refuses anything that is not a page object report", () => {
    expect(() => mapEnginePageObjects({ schema: "other/1", pages: [] })).toThrow(/page-objects/);
    expect(() => mapEnginePageObjects(null)).toThrow();
  });
});

describe("pageObjectsFromIr", () => {
  const model = pageObjectsFromIr(ir);

  it("turns positioned text blocks into text objects with bottom-left boxes", () => {
    expect(model.source).toBe("ir");
    expect(model.pages[0]).toEqual({ page: 1, widthPt: 595, heightPt: 842 });
    const first = model.objects.find((object) => object.kind === "text") as TextRunObject;
    // IR boxes measure from the top of the page; page objects measure from the bottom.
    const { left, top, right, bottom } = ir.layout?.blocks?.[first.id.slice(3)]?.bbox ?? {
      left: -1,
      top: -1,
      right: -1,
      bottom: -1,
    };
    expect(first.bbox).toEqual([left, 842 - bottom, right, 842 - top]);
    expect(first.id.startsWith("ir:")).toBe(true);
    expect(first.page).toBe(1);
  });

  it("treats fonts as subsets, so unseen characters are flagged", () => {
    const first = model.objects.find((object) => object.kind === "text") as TextRunObject;
    expect(first.isSubset).toBe(true);
    expect(first.embeddedGlyphs).toBeTruthy();
    expect(resolveFontForEdit(first, "ễ").substituted).toBe(true);
  });
});

describe("PageEditSession add_text", () => {
  const base: PageObject[] = [];

  it("sends font_name only when the user chose a font", () => {
    const session = new PageEditSession(base);
    session.addTextBox({ page: 1, x: 10, y: 10, text: "Default" });
    session.addTextBox({ page: 1, x: 10, y: 40, text: "Chosen", fontName: "Liberation Serif" });
    const ops = session.toEditPageParams().ops;
    expect(ops[0]).not.toHaveProperty("font_name");
    expect(ops[1]).toMatchObject({ op: "add_text", font_name: "Liberation Serif" });
  });
});

describe("editor data readers", () => {
  it("reads page sizes, form fields and annotations from tool data", () => {
    expect(
      pagesFromInfo({ pages: [{ page: 1, width_pt: 612, height_pt: 792 }, { page: 2 }] }),
    ).toEqual([{ page: 1, widthPt: 612, heightPt: 792 }]);
    expect(
      fieldsFromList({
        fields: [
          { name: "a", type: "text", page: 1, rect: [1, 2, 3, 4], value: "" },
          { name: "b", type: "unknown", page: 1, rect: [1, 2, 3, 4] },
          { name: "c", type: "text", page: 1, rect: [1, 2, 3] },
        ],
      }).map((field) => field.name),
    ).toEqual(["a"]);
    expect(
      annotationsFromList({
        annotations: [
          { id: "annotation-0", page: 1, type: "highlight", rect: [1, 2, 3, 4] },
          { id: "annotation-1", page: 1, type: null },
        ],
      }),
    ).toEqual([{ id: "annotation-0", page: 1, type: "highlight", rect: [1, 2, 3, 4] }]);
  });

  it("opens Document mode at the shared position, or a margin corner", () => {
    expect(initialPointFor({ page: 3, xPt: 10, yPt: 20 })).toEqual({ pageNo: 3, x: 10, y: 20 });
    expect(initialPointFor({ page: 2 })).toEqual({ pageNo: 2, x: 72, y: 72 });
  });

  it("follows the selected block and keeps the old position when nothing is selected", () => {
    const before = { page: 1 };
    const selected = locationFromSelection(
      {
        blockId: "b1",
        location: { page_no: 4, bbox: { left: 54, top: 88, right: 540, bottom: 104 } },
      },
      before,
    );
    expect(selected).toEqual({ page: 4, xPt: 54, yPt: 88, blockId: "b1" });
    expect(locationFromSelection({ blockId: null, location: null }, selected)).toBe(selected);
  });
});

function irOutput(document: IrDocument): ConvertedFile {
  const blob = new Blob([JSON.stringify(document)], { type: "application/json" });
  return {
    name: "report.json",
    size: blob.size,
    format: "ariad-ir+json",
    mediaType: "application/json",
    route: [],
    warnings: [],
    elapsedMs: 1,
    location: { kind: "memory", blob },
    recycle: false,
  };
}

async function openWorkspace(extra: Partial<React.ComponentProps<typeof DocumentWorkspace>> = {}) {
  const convert = vi.fn(async () => irOutput(ir));
  const client = fakeClient({ convert });
  const viewer = fakeBackend();
  const view = render(<DocumentWorkspace client={client} loadBackend={viewer.load} {...extra} />);
  const input = view.container.querySelector("input[type=file]") as HTMLInputElement;
  await act(async () => {
    fireEvent.change(input, { target: { files: [pdfFile("report.pdf")] } });
  });
  await screen.findByText("What is in this PDF");
  return { client, convert, viewer, view };
}

describe("<DocumentWorkspace /> editor modes", () => {
  it("offers View, Annotate, Document and Page once a PDF is open", async () => {
    await openWorkspace();
    const group = screen.getByTestId("workspace-mode-switch");
    expect(group.querySelectorAll("button")).toHaveLength(4);
    expect(screen.getByTestId("workspace-mode-view").getAttribute("aria-pressed")).toBe("true");
    for (const mode of ["annotate", "document", "page"] as const) {
      expect(screen.getByTestId(`workspace-mode-${mode}`)).toBeTruthy();
    }
  });

  it("does not show the switch without a document", () => {
    render(<DocumentWorkspace client={fakeClient()} loadBackend={fakeBackend().load} />);
    expect(screen.queryByTestId("workspace-mode-switch")).toBeNull();
  });

  it("converts the active PDF to IR when Document mode opens, and shows the editor", async () => {
    const { convert } = await openWorkspace();
    fireEvent.click(screen.getByTestId("workspace-mode-document"));
    await waitFor(() => expect(screen.getByTestId("document-mode-editor")).toBeTruthy());
    expect(convert).toHaveBeenCalledWith("ariad-ir+json", "editable", { inputId: "slot-1" });
    expect(screen.getByTestId("workspace-mode-document").getAttribute("aria-pressed")).toBe("true");
  });

  it("keeps the edit session when the user leaves Document mode and returns", async () => {
    const { convert } = await openWorkspace();
    fireEvent.click(screen.getByTestId("workspace-mode-document"));
    await screen.findByTestId("document-mode-editor");
    fireEvent.click(screen.getByTestId("workspace-mode-view"));
    await waitFor(() => expect(screen.queryByTestId("document-mode-editor")).toBeNull());
    fireEvent.click(screen.getByTestId("workspace-mode-document"));
    await screen.findByTestId("document-mode-editor");
    expect(convert).toHaveBeenCalledTimes(1);
  });

  it("reports why the editor could not open a document and tries again", async () => {
    const { convert } = await openWorkspace();
    convert.mockRejectedValueOnce({ code: "bad_pdf", message: "Bad xref" });
    fireEvent.click(screen.getByTestId("workspace-mode-document"));
    const alert = await screen.findByText(/could not open this document\. Bad xref/);
    expect(alert).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Try again" }));
    await screen.findByTestId("document-mode-editor");
  });

  it("opens Page mode with objects estimated from the layout and says so", async () => {
    await openWorkspace();
    fireEvent.click(screen.getByTestId("workspace-mode-page"));
    await screen.findByTestId("page-objects-estimated");
    expect(screen.getByTestId("page-objects-estimated").textContent).toMatch(/PDFium engine/);
  });

  it("reads forms and annotations for the Annotate surface", async () => {
    const runTool = vi.fn(async (toolId: string) => ({
      tool: toolId,
      format: "json",
      mediaType: "application/json",
      extension: "json",
      primaryLocation: { kind: "memory" as const, blob: new Blob([]) },
      artifacts: [],
      data:
        toolId === "pdf.info"
          ? { pages: [{ page: 1, width_pt: 612, height_pt: 792 }] }
          : toolId === "pdf.forms_list"
            ? { fields: [] }
            : { annotations: [] },
      warnings: [],
      metrics: {},
      elapsedMs: 1,
      recycle: false,
    }));
    const { client } = await openWorkspace();
    client.runTool.mockImplementation(runTool as never);
    fireEvent.click(screen.getByTestId("workspace-mode-annotate"));
    await waitFor(() =>
      expect(document.querySelector('[data-slot="annotate-forms-surface"]')).toBeTruthy(),
    );
    expect(runTool).toHaveBeenCalledWith(
      "pdf.info",
      {},
      expect.objectContaining({ inputIds: ["slot-1"] }),
    );
    expect(runTool).toHaveBeenCalledWith("pdf.annotations", { action: "list" }, expect.anything());
  });

  it("shows the AI panel offline-first behind its own tab, without running anything", async () => {
    const { client } = await openWorkspace();
    expect(document.querySelector('[data-slot="ai-panel"]')).toBeNull();
    fireEvent.click(screen.getByTestId("inspector-tab-ai"));
    const badge = await waitFor(() => {
      const found = document.querySelector('[data-slot="ai-privacy-badge"]') as HTMLElement;
      expect(found).toBeTruthy();
      return found;
    });
    expect(badge.textContent).toContain("0 bytes uploaded");
    expect(badge.getAttribute("data-provider")).toBe("local");
    // The browser has no local model, so the panel offers the Desktop command, not a Run button.
    expect(document.querySelector('[data-slot="ai-handoff"]')).toBeTruthy();
    expect(client.runTool).not.toHaveBeenCalledWith(
      expect.stringMatching(/^ai\./),
      expect.anything(),
      expect.anything(),
    );
  });

  it("hands an AI recipe draft to the host as steps, never running it", async () => {
    const onOpenRecipeDraft = vi.fn();
    const { client } = await openWorkspace({ onOpenRecipeDraft });
    const before = client.runTool.mock.calls.length;
    fireEvent.click(screen.getByTestId("inspector-tab-ai"));
    fireEvent.click(await screen.findByRole("tab", { name: "Recipe" }));
    fireEvent.change(screen.getByLabelText("Describe the steps"), {
      target: { value: "merge, OCR, compress under 5 MB" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Draft recipe" }));
    fireEvent.click(await screen.findByRole("button", { name: /Open in Recipe Builder/ }));
    expect(onOpenRecipeDraft).toHaveBeenCalledTimes(1);
    const steps = onOpenRecipeDraft.mock.calls[0]?.[0].steps as Array<{ tool: string }>;
    expect(steps.map((step) => step.tool)).toEqual(["pdf.merge", "pdf.ocr", "pdf.compress"]);
    expect(client.runTool.mock.calls.length).toBe(before);
  });
});

describe("<RecipesHost /> imported recipes", () => {
  const drafted = JSON.stringify({
    version: "ariad-recipe/1",
    steps: [{ tool: "pdf.compress", params: { preset: "ebook" } }],
  });

  it("holds an AI draft behind the confirmation gate until the user agrees", () => {
    const onResolved = vi.fn();
    render(<RecipesHost draft={null} fragment={null} imported={drafted} onResolved={onResolved} />);
    expect(screen.getByRole("dialog")).toBeTruthy();
    expect(document.querySelector('[data-slot="recipe-step"]')).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Confirm & Load" }));
    expect(onResolved).toHaveBeenCalledOnce();
    expect(document.querySelectorAll('[data-slot="recipe-step"]')).toHaveLength(1);
  });

  it("refuses a draft that carries a field the recipe format does not have", () => {
    const named = JSON.stringify({ ...JSON.parse(drafted), name: "Mine" });
    render(<RecipesHost draft={null} fragment={null} imported={named} onResolved={vi.fn()} />);
    expect(document.querySelector('[data-slot="recipe-link-problem"]')).toBeTruthy();
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
