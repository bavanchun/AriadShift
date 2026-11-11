// @vitest-environment jsdom

import { readdirSync, readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import {
  contentAssetId,
  DocumentEditSession,
  DocumentModeEditor,
  type DocumentModeEditorProps,
  defaultDocumentModeLabels,
  ExportFidelityDialog,
  type ExportFormat,
  type ExportRequest,
} from "@/editor/document";
import { readDomSelection, writeDomSelection } from "@/editor/document/inline-dom";
import {
  type IrBlock,
  type IrDocument,
  irToEditorState,
  summarizeEditorChanges,
} from "@/editor/document/mapping";
import { createEditorVersionStore, MemoryVersionBackend } from "@/editor/storage";

afterEach(cleanup);

const fixtureRoot = resolve(import.meta.dirname, "../../../fixtures/editor");
const fixtureNames = readdirSync(fixtureRoot).filter((name) => name.endsWith(".ir.json"));
const readFixture = (name: string): IrDocument =>
  JSON.parse(readFileSync(join(fixtureRoot, name), "utf8"));

const para = (id: string, text: string, extra: Record<string, unknown> = {}): IrBlock => ({
  type: "paragraph",
  id,
  content: [{ type: "text", text }],
  ...extra,
});
const heading = (id: string, level: number, text: string): IrBlock => ({
  type: "heading",
  id,
  level,
  content: [{ type: "text", text }],
});
const makeIr = (body: IrBlock[], extra: Partial<IrDocument> = {}): IrDocument => ({
  version: "ariad-ir/2",
  meta: {},
  body,
  ...extra,
});
const newSession = (ir: IrDocument) => new DocumentEditSession(irToEditorState(ir));
const range = (blockId: string, from: number, to: number) => ({
  blockId,
  part: "content" as const,
  from,
  to,
});
const textOfInlines = (content: unknown): string =>
  ((content as { type: string; text?: string; content?: unknown[] }[]) ?? [])
    .map((node) => node.text ?? textOfInlines(node.content))
    .join("");

/** Every section must cover each of the table's columns exactly once, within its own rows. */
function expectRectangular(table: IrBlock | undefined) {
  const width = table?.columns?.length ?? 0;
  for (const rows of [table?.head ?? [], table?.body ?? []]) {
    const grid = rows.map(() => Array.from({ length: width }, () => 0));
    for (const [r, row] of rows.entries()) {
      let c = 0;
      for (const cell of row) {
        while (c < width && grid[r]?.[c]) c++;
        for (let dr = 0; dr < cell.rowspan; dr++) {
          for (let dc = 0; dc < cell.colspan; dc++) {
            const line = grid[r + dr];
            expect(line, "a span must stay inside its section").toBeDefined();
            expect(c + dc).toBeLessThan(width);
            if (line) line[c + dc] = (line[c + dc] ?? 0) + 1;
          }
        }
        c += cell.colspan;
      }
    }
    for (const line of grid) expect(line).toEqual(Array.from({ length: width }, () => 1));
  }
}

const assetIdOf = (block: IrBlock | undefined): string =>
  block?.asset?.type === "asset" ? block.asset.id : "";

const styledIr = () =>
  makeIr(
    [
      para("p1", "Hello world", {
        style: "body",
        direct: { align: "center" },
        extension: { keep: true },
      }),
      para("p2", "Second"),
      para("p3", "Third"),
    ],
    {
      styles: {
        paragraph: { body: { size_pt: 11, font: { family: "Libertinus Serif" } } },
        character: { accent: { color: "#185c4c", italic: true } },
      },
    },
  );

const laidOut = () =>
  makeIr([para("a", "Alpha"), para("b", "Beta"), para("c", "Gamma")], {
    layout: {
      pages: [
        { page_no: 1, width: 595, height: 842 },
        { page_no: 2, width: 595, height: 842 },
      ],
      blocks: {
        a: { page_no: 1, bbox: { left: 50, top: 50, right: 300, bottom: 90 } },
        b: { page_no: 1, bbox: { left: 50, top: 200, right: 300, bottom: 240 } },
        c: { page_no: 2, bbox: { left: 50, top: 50, right: 300, bottom: 90 } },
      },
    },
  });

function setup(ir: IrDocument, props: Partial<DocumentModeEditorProps> = {}) {
  const session = newSession(ir);
  const backend = new MemoryVersionBackend();
  const store = createEditorVersionStore({ backend });
  const view = render(
    <DocumentModeEditor
      docId="doc-1"
      session={session}
      store={store}
      autosaveIntervalMs={0}
      {...props}
    />,
  );
  return { session, store, backend, ...view };
}

const editable = (container: HTMLElement, blockId: string, part = "content") => {
  const element = container.querySelector<HTMLElement>(
    `[data-inline-editable][data-block-id="${blockId}"][data-part="${part}"]`,
  );
  if (!element) throw new Error(`No editable for ${blockId}`);
  return element;
};

function firstTextNode(element: HTMLElement): Text {
  const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
  const node = walker.nextNode();
  if (!node) throw new Error("No text node");
  return node as Text;
}

function selectRange(element: HTMLElement, from: number, to = from) {
  act(() => {
    element.focus();
    writeDomSelection(element, from, to);
    fireEvent.keyUp(element);
  });
}

describe("DocumentEditSession: identity and lossless export", () => {
  it.each(fixtureNames)("exports %s unchanged when nothing was edited", (name) => {
    const ir = readFixture(name);
    const session = newSession(ir);
    expect(session.exportIr()).toEqual(ir);
    const summary = session.getChangeSummary();
    expect(summary.addedBlocks).toEqual([]);
    expect(summary.modifiedBlocks).toEqual([]);
    expect(summary.deletedBlocks).toEqual([]);
    expect(summary.styleChanges).toEqual([]);
    expect(summary.estimatedFidelityScore).toBe(1);
  });

  it("edits text while keeping blockId, style, direct and extension fields, and leaves neighbours byte-identical", () => {
    const ir = styledIr();
    const session = newSession(ir);
    expect(session.replaceText("p1", "content", 0, 5, "Howdy")).toBe(true);
    const out = session.exportIr();
    expect(out.body[0]).toEqual({
      ...ir.body[0],
      content: [{ type: "text", text: "Howdy world" }],
    });
    expect(out.body[1]).toEqual(ir.body[1]);
    expect(out.body[2]).toEqual(ir.body[2]);
    expect(session.getChangeSummary().modifiedBlocks).toEqual(["p1"]);
  });

  it("keeps assets, layout and furniture out-of-band data untouched after an edit", () => {
    const ir = readFixture("mixed-document.ir.json");
    const session = newSession(ir);
    const target = ir.body.find((block) => block.type === "paragraph" && block.id);
    session.replaceText(String(target?.id), "content", 0, 0, "X");
    const out = session.exportIr();
    expect(out.assets).toEqual(ir.assets);
    expect(out.styles).toEqual(ir.styles);
    expect(out.sections).toEqual(ir.sections);
    expect(out.layout).toEqual(ir.layout);
    expect(out.source_map).toEqual(ir.source_map);
  });

  it("serializes the exported IR as JSON", () => {
    const session = newSession(styledIr());
    session.replaceText("p2", "content", 0, 6, "Changed");
    expect(JSON.parse(session.serializeIr())).toEqual(session.exportIr());
  });
});

describe("DocumentEditSession: inline marks", () => {
  const ir = () => makeIr([para("p1", "Hello world")]);

  it("adds and removes bold losslessly", () => {
    const original = ir();
    const session = newSession(original);
    session.toggleMark("bold", { range: range("p1", 6, 11) });
    expect(session.exportIr().body[0]?.content).toEqual([
      { type: "text", text: "Hello " },
      { type: "strong", content: [{ type: "text", text: "world" }] },
    ]);
    session.toggleMark("bold", { range: range("p1", 6, 11) });
    expect(session.exportIr()).toEqual(original);
  });

  it.each([
    ["italic", "emph"],
    ["strike", "strikeout"],
    ["subscript", "subscript"],
    ["superscript", "superscript"],
  ] as const)("maps %s to the IR %s inline", (kind, irType) => {
    const session = newSession(ir());
    session.toggleMark(kind, { range: range("p1", 0, 5) });
    expect(session.exportIr().body[0]?.content?.[0]).toEqual({
      type: irType,
      content: [{ type: "text", text: "Hello" }],
    });
  });

  it("maps underline to direct character formatting and code to a code inline", () => {
    const session = newSession(ir());
    session.toggleMark("underline", { range: range("p1", 0, 5) });
    session.toggleMark("code", { range: range("p1", 6, 11) });
    expect(session.exportIr().body[0]?.content).toEqual([
      { type: "styled", direct: { underline: true }, content: [{ type: "text", text: "Hello" }] },
      { type: "text", text: " " },
      { type: "code", text: "world" },
    ]);
  });

  it("makes subscript and superscript mutually exclusive", () => {
    const session = newSession(ir());
    session.toggleMark("subscript", { range: range("p1", 0, 5) });
    session.toggleMark("superscript", { range: range("p1", 0, 5) });
    expect(session.exportIr().body[0]?.content?.[0]?.type).toBe("superscript");
  });

  it("sets and removes links with href and title", () => {
    const session = newSession(ir());
    session.setLink("https://example.com", "Example", range("p1", 6, 11));
    expect(session.exportIr().body[0]?.content?.[1]).toEqual({
      type: "link",
      url: "https://example.com",
      title: "Example",
      content: [{ type: "text", text: "world" }],
    });
    session.removeLink(range("p1", 6, 11));
    expect(session.exportIr().body[0]?.content).toEqual([{ type: "text", text: "Hello world" }]);
  });

  it("applies direct character styles and named character styles", () => {
    const session = newSession(styledIr());
    session.applyCharacterDirect({ size_pt: 14, font: { family: "Noto Sans" } }, range("p2", 0, 3));
    session.applyCharacterStyle("accent", range("p3", 0, 5));
    const out = session.exportIr();
    expect(out.body[1]?.content?.[0]).toEqual({
      type: "styled",
      direct: { size_pt: 14, font: { family: "Noto Sans" } },
      content: [{ type: "text", text: "Sec" }],
    });
    expect(out.body[2]?.content).toEqual([
      { type: "styled", style: "accent", content: [{ type: "text", text: "Third" }] },
    ]);
    expect(session.getChangeSummary().styleChanges).toEqual(expect.arrayContaining(["p2", "p3"]));
  });

  it("formats the whole block when only a caret is placed", () => {
    const session = newSession(ir());
    session.selectBlock("p1");
    session.toggleMark("italic");
    expect(session.exportIr().body[0]?.content).toEqual([
      { type: "emph", content: [{ type: "text", text: "Hello world" }] },
    ]);
  });

  it("clears direct bold when the text is bold only through an enclosing span's weight", () => {
    const original = readFixture("styled-nesting.ir.json");
    const session = newSession(original);
    const paragraphId = String(original.body[2]?.blocks?.[0]?.items?.[0]?.blocks[0]?.id);
    const text = "Giữ nguyên tiếng Việt: Nguyễn, đường, ươm. Nhấn mạnh bên trong.";
    const from = text.indexOf("Nhấn");
    session.toggleMark("bold", { range: range(paragraphId, from, from + 4) });
    const spans = session.exportIr().body[2]?.blocks?.[0]?.items?.[0]?.blocks[0]?.content ?? [];
    expect(spans).toHaveLength(3);
    expect(spans[1]).toMatchObject({ style: "accent", direct: { italic: false } });
    expect(spans[1]?.direct).not.toHaveProperty("weight");
    expect(spans[2]).toMatchObject({ direct: { italic: false, weight: 650 } });
    expect(textOfInlines(spans)).toBe(text);
  });

  it("keeps repeated nested original spans and their extension fields when adding a mark inside them", () => {
    const original = readFixture("styled-nesting.ir.json");
    const session = newSession(original);
    const list = original.body[2]?.blocks?.[0];
    const paragraphId = String(list?.items?.[0]?.blocks[0]?.id);
    const text = "Giữ nguyên tiếng Việt: Nguyễn, đường, ươm. Nhấn mạnh bên trong.";
    const from = text.indexOf("Nhấn");
    session.toggleMark("italic", { range: range(paragraphId, from, from + 4) });
    const out = session.exportIr();
    const outer = out.body[2]?.blocks?.[0]?.items?.[0]?.blocks[0]?.content?.[0];
    expect(outer).toMatchObject({
      type: "styled",
      style: "accent",
      direct: { italic: false, weight: 650 },
    });
    const inner = (outer?.content as IrBlock[])?.[1];
    expect(inner).toMatchObject({ type: "styled", direct: { size_pt: 13, underline: true } });
    expect(JSON.stringify(inner?.content)).toContain('"emph"');
    expect(textOfInlines(outer?.content)).toBe(text);
  });

  it("preserves the text of out-of-band inline atoms when editing around them", () => {
    const original = readFixture("inline-math.ir.json");
    const session = newSession(original);
    const target = original.body[2] as IrBlock;
    session.replaceText(target.id, "content", 0, 0, ">> ");
    const content = session.exportIr().body[2]?.content ?? [];
    expect(content[0]).toEqual({
      type: "text",
      text: `>> ${textOfInlines(target.content?.slice(0, 1))}`,
    });
    expect(content.slice(1)).toEqual(target.content?.slice(1));
  });
});

describe("DocumentEditSession: blocks", () => {
  it("changes paragraph to heading levels 1-6 and back, keeping the block ID", () => {
    const session = newSession(makeIr([para("p1", "Title"), para("p2", "Body")]));
    for (const level of [1, 2, 3, 4, 5, 6]) {
      expect(session.setBlockType(`heading-${level}` as "heading-1", "p1")).toBe(true);
      expect(session.exportIr().body[0]).toMatchObject({ id: "p1", type: "heading", level });
    }
    session.setBlockType("paragraph", "p1");
    expect(session.exportIr().body[0]).toEqual(para("p1", "Title"));
  });

  it("wraps a paragraph in a quote and lifts it back out", () => {
    const original = makeIr([para("p1", "Quoted"), para("p2", "Other")]);
    const session = newSession(original);
    session.setBlockType("quote", "p1");
    const wrapped = session.exportIr().body[0];
    expect(wrapped?.type).toBe("quote");
    expect(wrapped?.blocks?.[0]).toEqual(para("p1", "Quoted"));
    session.setBlockType("paragraph", "p1");
    expect(session.exportIr().body).toEqual(original.body);
  });

  it("converts to and from code blocks keeping the text", () => {
    const session = newSession(makeIr([para("p1", "let x = 1;")]));
    session.setBlockType("code", "p1");
    expect(session.exportIr().body[0]).toMatchObject({
      type: "code",
      text: "let x = 1;",
      lang: null,
    });
    session.setBlockType("paragraph", "p1");
    expect(session.exportIr().body[0]).toMatchObject({
      type: "paragraph",
      content: [{ type: "text", text: "let x = 1;" }],
    });
  });

  it("turns a paragraph into bulleted and numbered lists and flips between them", () => {
    const session = newSession(makeIr([para("p1", "Item")]));
    session.setBlockType("bulletList", "p1");
    let list = session.exportIr().body[0];
    expect(list).toMatchObject({ type: "list", ordered: false });
    expect(list?.items?.[0]?.blocks[0]).toEqual(para("p1", "Item"));
    session.setBlockType("orderedList", "p1");
    list = session.exportIr().body[0];
    expect(list).toMatchObject({ type: "list", ordered: true });
    session.setBlockType("paragraph", "p1");
    expect(session.exportIr().body).toEqual([para("p1", "Item")]);
  });

  it("inserts blocks with fresh, unique, valid IDs", () => {
    const session = newSession(makeIr([para("p1", "A"), para("p2", "B")]));
    const ids = [
      session.insertBlock({ kind: "paragraph", text: "new" }, { afterId: "p1" }),
      session.insertBlock({ kind: "heading", level: 2, text: "H" }, { afterId: "p1" }),
      session.insertBlock({ kind: "mathBlock", tex: "x^2" }),
      session.insertBlock({ kind: "horizontalRule" }),
      session.insertBlock({ kind: "table", rows: 3, columns: 2, header: true }),
    ];
    expect(ids.every((id) => id && /^[A-Za-z][A-Za-z0-9_.-]*$/.test(id))).toBe(true);
    expect(new Set(ids).size).toBe(5);
    const out = session.exportIr();
    // Each insert lands after the block selected by the previous one.
    expect(out.body.map((block) => block.type)).toEqual([
      "paragraph",
      "heading",
      "math",
      "paragraph",
      "table",
      "paragraph",
      "paragraph",
    ]);
    // The IR has no rule block: a rule is the paragraph the core's readers produce for one.
    expect(textOfInlines(out.body[3]?.content)).toBe("———");
    expect(session.getChangeSummary().addedBlocks).toEqual(expect.arrayContaining(ids as string[]));
  });

  it("builds tables with header row, columns and unit spans", () => {
    const session = newSession(makeIr([]));
    session.insertBlock({ kind: "table", rows: 3, columns: 2, header: true });
    const table = session.exportIr().body[0];
    expect(table?.head).toHaveLength(1);
    expect(table?.body).toHaveLength(2);
    expect(table?.columns).toHaveLength(2);
    const cells = [...(table?.head ?? []), ...(table?.body ?? [])].flat();
    expect(cells).toHaveLength(6);
    expect(cells.every((cell) => cell.rowspan === 1 && cell.colspan === 1)).toBe(true);
    expect(table?.head?.[0]?.every((cell) => cell.header === true)).toBe(true);
  });

  it("keeps the grid rectangular when merging, splitting and resizing cells", () => {
    const session = newSession(makeIr([]));
    const id = String(session.insertBlock({ kind: "table", rows: 4, columns: 3, header: true }));
    const table = () => session.exportIr().body[0] as IrBlock;
    const cellRef = { section: "body", row: 0, column: 0 } as const;
    expectRectangular(table());
    // Merging swallows the covered cells.
    expect(session.setTableCellSpan(id, cellRef, { colspan: 2, rowspan: 2 })).toBe(true);
    expect(table().body?.[0]?.map((cell) => [cell.rowspan, cell.colspan])).toEqual([
      [2, 2],
      [1, 1],
    ]);
    expect(table().body?.[1]).toHaveLength(1);
    expectRectangular(table());
    // Spans are clamped to the grid.
    session.setTableCellSpan(id, cellRef, { colspan: 9, rowspan: 9 });
    expect(table().body?.[0]?.[0]).toMatchObject({ colspan: 3, rowspan: 3 });
    expectRectangular(table());
    // Splitting recreates the released cells.
    session.setTableCellSpan(id, cellRef, { colspan: 1, rowspan: 1 });
    expect(table().body?.map((row) => row.length)).toEqual([3, 3, 3]);
    expectRectangular(table());
    // A span that would cut through another merged cell is refused.
    session.setTableCellSpan(id, { section: "body", row: 0, column: 1 }, { colspan: 2 });
    expect(session.setTableCellSpan(id, cellRef, { colspan: 2 })).toBe(false);
    expectRectangular(table());
  });

  it("keeps the grid rectangular when rows and columns change around merged cells", () => {
    const session = newSession(makeIr([]));
    const id = String(session.insertBlock({ kind: "table", rows: 4, columns: 3, header: true }));
    const table = () => session.exportIr().body[0] as IrBlock;
    session.setTableCellSpan(
      id,
      { section: "body", row: 0, column: 0 },
      { colspan: 2, rowspan: 2 },
    );
    // A row added inside the merged cell extends it instead of adding a cell under it.
    expect(session.addTableRow(id, "body", 0)).toBe(true);
    expect(table().body?.[0]?.[0]).toMatchObject({ rowspan: 3, colspan: 2 });
    expect(table().body?.[1]).toHaveLength(1);
    expectRectangular(table());
    // Deleting a row hands its merged cells down to the next row.
    expect(session.deleteTableRow(id, { section: "body", row: 0 })).toBe(true);
    expect(table().body?.[0]?.[0]).toMatchObject({ rowspan: 2, colspan: 2 });
    expectRectangular(table());
    // Deleting a column narrows merged cells that cross it.
    expect(session.deleteTableColumn(id, { section: "body", row: 0, column: 0 })).toBe(true);
    expect(table().body?.[0]?.[0]).toMatchObject({ colspan: 1 });
    expect(table().columns).toHaveLength(2);
    expectRectangular(table());
    expect(session.addTableColumn(id)).toBe(true);
    expect(table().columns).toHaveLength(3);
    expectRectangular(table());
    // The last column and the last row cannot be removed.
    const small = String(
      session.insertBlock({ kind: "table", rows: 1, columns: 1, header: false }),
    );
    expect(session.deleteTableColumn(small, { section: "body", row: 0, column: 0 })).toBe(false);
    expect(session.deleteTableRow(small, { section: "body", row: 0 })).toBe(false);
  });

  it("toggles the header row and keeps spanned fixture tables rectangular through edits", () => {
    const session = newSession(readFixture("table-combined-spans.ir.json"));
    const id = String(
      session.getState().doc.content.find((node) => node.type === "table")?.attrs?.blockId,
    );
    const table = () => session.exportIr().body.find((block) => block.type === "table") as IrBlock;
    expectRectangular(table());
    expect(session.toggleTableHeader(id)).toBe(true);
    expect(table().head).toEqual([]);
    expect(table().body).toHaveLength(3);
    expect(table().body?.[0]?.every((cell) => cell.header === false)).toBe(true);
    expectRectangular(table());
    // The first body row now carries a rowspan, which a header row cannot continue into the body.
    expect(session.deleteTableRow(id, { section: "body", row: 0 })).toBe(true);
    expectRectangular(table());
    expect(session.toggleTableHeader(id)).toBe(false);
    session.undo();
    session.undo();
    session.addTableRow(id, "head", 0);
    session.addTableColumn(id);
    expectRectangular(table());
  });

  it("adds a header row to a table that has none", () => {
    const session = newSession(makeIr([]));
    const id = String(session.insertBlock({ kind: "table", rows: 2, columns: 2, header: false }));
    expect(session.toggleTableHeader(id)).toBe(true);
    const table = session.exportIr().body[0] as IrBlock;
    expect(table.head).toHaveLength(1);
    expect(table.head?.[0]?.every((cell) => cell.header === true)).toBe(true);
    expect(table.body).toHaveLength(1);
  });

  it("registers image bytes as an asset and inserts a captioned figure", () => {
    const session = newSession(makeIr([para("p1", "A")]));
    const id = "a".repeat(64);
    const figureId = session.insertImage(
      { id, base64: "AAEC", mediaType: "image/png" },
      "A caption",
    );
    const out = session.exportIr();
    const figure = out.body.find((block) => block.id === figureId);
    expect(figure).toMatchObject({
      type: "figure",
      caption: [{ type: "text", text: "A caption" }],
    });
    expect(assetIdOf(figure)).toBe(id);
    expect(out.assets?.[id]).toEqual({ bytes: "AAEC", media_type: "image/png" });
    // The same bytes reuse the asset instead of storing a second copy.
    session.insertImage({ id, base64: "AAEC", mediaType: "image/png" });
    expect(Object.keys(session.exportIr().assets ?? {})).toEqual([id]);
  });

  it("derives asset IDs from the SHA-256 of the bytes", async () => {
    expect(await contentAssetId(new Uint8Array([1, 2, 3]))).toBe(
      "039058c6f2c0cb492c533b0a4d14ef77cc0f78abccced5287d84a1a2011cfb81",
    );
    expect(await contentAssetId(new Uint8Array())).toBe(
      "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
  });

  it("inserts a footnote reference and appends the footnote block", () => {
    const session = newSession(makeIr([para("p1", "Cited")]));
    session.setSelection(range("p1", 5, 5));
    const created = session.insertFootnote("Source");
    expect(created).not.toBeNull();
    const out = session.exportIr();
    expect(out.body[0]?.content?.[1]).toEqual({ type: "footnote_ref", id: created?.noteId });
    expect(out.body[1]).toMatchObject({ type: "footnote", note_id: created?.noteId });
  });

  it("edits math TeX of a math block and an inline math atom", () => {
    const original = readFixture("display-math.ir.json");
    const session = newSession(original);
    const math = original.body.find((block) => block.type === "math");
    session.setMathSource(String(math?.id), "a+b", false);
    expect(session.exportIr().body[2]).toMatchObject({ type: "math", tex: "a+b", display: false });
    const inline = readFixture("inline-math.ir.json");
    const second = newSession(inline);
    const target = inline.body[2] as IrBlock;
    const offset = (target.content ?? [])
      .slice(0, 1)
      .reduce((n, node) => n + (node.text?.length ?? 0), 0);
    session.setSelection(null);
    expect(second.updateInlineAtom(target.id, "content", offset, { tex: "z" })).toBe(true);
    expect(JSON.stringify(second.exportIr().body[2]?.content)).toContain('"tex":"z"');
  });

  it("deletes, moves and duplicates blocks", () => {
    const session = newSession(makeIr([para("p1", "A"), para("p2", "B"), para("p3", "C")]));
    session.moveBlock(1, "p1");
    expect(session.exportIr().body.map((block) => block.id)).toEqual(["p2", "p1", "p3"]);
    const copy = session.duplicateBlock("p3");
    expect(copy).toMatch(/^bEditor/);
    expect(session.exportIr().body.map((block) => textOfInlines(block.content))).toEqual([
      "B",
      "A",
      "C",
      "C",
    ]);
    session.deleteBlock("p2");
    const out = session.exportIr();
    expect(out.body.map((block) => block.id)).toEqual(["p1", "p3", copy]);
    expect(session.getChangeSummary().deletedBlocks).toEqual(["p2"]);
    expect(session.moveBlock(-1, "p1")).toBe(false);
  });

  it("deleting the only block of a table cell leaves an empty paragraph", () => {
    const session = newSession(readFixture("table-colspan.ir.json"));
    const table = session.getState().doc.content.find((node) => node.type === "table");
    const cellBlock = table?.content?.[0]?.content?.[0]?.content?.[0];
    expect(session.deleteBlock(String(cellBlock?.attrs?.blockId))).toBe(true);
    const out = session.exportIr();
    const cell = out.body.find((block) => block.type === "table")?.head?.[0]?.[0];
    expect(cell?.blocks).toHaveLength(1);
    expect(cell?.blocks[0]?.content).toEqual([]);
  });

  it("splits a block at the caret and merges it back", () => {
    const session = newSession(makeIr([para("p1", "Hello world"), para("p2", "Tail")]));
    const created = session.splitBlock("p1", 5);
    expect(created).toMatch(/^bEditor/);
    expect(session.exportIr().body.map((block) => textOfInlines(block.content))).toEqual([
      "Hello",
      " world",
      "Tail",
    ]);
    expect(session.getSelectedBlockId()).toBe(created);
    expect(session.mergeWithPrevious(String(created))).toBe(true);
    expect(session.exportIr().body[0]).toEqual(para("p1", "Hello world"));
    expect(session.mergeWithNext("p1")).toBe(true);
    expect(textOfInlines(session.exportIr().body[0]?.content)).toBe("Hello worldTail");
  });

  it("an Enter at the end of a heading starts a plain paragraph", () => {
    const session = newSession(makeIr([heading("h1", 1, "Title")]));
    session.splitBlock("h1", 5);
    expect(session.exportIr().body.map((block) => block.type)).toEqual(["heading", "paragraph"]);
  });

  it("an Enter in a list item starts a new list item", () => {
    const session = newSession(makeIr([para("p1", "One two")]));
    session.setBlockType("bulletList", "p1");
    session.splitBlock("p1", 3);
    const list = session.exportIr().body[0];
    expect(list?.items).toHaveLength(2);
    expect(textOfInlines(list?.items?.[1]?.blocks[0]?.content)).toBe(" two");
  });
});

describe("DocumentEditSession: sections", () => {
  it("re-anchors a deleted section start to the block that followed it", () => {
    const original = readFixture("mixed-document.ir.json");
    const startId = String(original.sections?.[1]?.start);
    const index = original.body.findIndex((block) => block.id === startId);
    const session = newSession(original);
    expect(session.deleteBlock(startId)).toBe(true);
    const out = session.exportIr();
    expect(out.sections?.[1]?.start).toBe(original.body[index + 1]?.id);
    expect(out.sections?.[0]).toEqual(original.sections?.[0]);
  });

  it("keeps sections valid when a section start is moved", () => {
    const original = readFixture("mixed-document.ir.json");
    const startId = String(original.sections?.[1]?.start);
    const session = newSession(original);
    session.moveBlock(-1, startId);
    expect(() => session.exportIr()).not.toThrow();
    session.moveBlock(1, startId);
    expect(() => session.exportIr()).not.toThrow();
  });

  it("moves a section start into a wrapper when its block changes type", () => {
    const original = readFixture("mixed-document.ir.json");
    const startId = String(original.sections?.[1]?.start);
    const start = original.body.find((block) => block.id === startId);
    const session = newSession(original);
    if (start?.type === "paragraph") {
      session.setBlockType("quote", startId);
      const out = session.exportIr();
      const wrapper = out.body.find((block) => block.blocks?.[0]?.id === startId);
      expect(out.sections?.[1]?.start).toBe(wrapper?.id);
    } else {
      expect(() => session.exportIr()).not.toThrow();
    }
  });
});

describe("DocumentEditSession: history", () => {
  it("undoes and redoes edits, and a new edit clears the redo stack", () => {
    const original = makeIr([para("p1", "A"), para("p2", "B")]);
    const session = newSession(original);
    expect(session.canUndo).toBe(false);
    session.insertBlock({ kind: "paragraph", text: "x" }, { afterId: "p1" });
    session.deleteBlock("p2");
    expect(session.canUndo).toBe(true);
    session.undo();
    session.undo();
    expect(session.exportIr()).toEqual(original);
    expect(session.canRedo).toBe(true);
    session.redo();
    expect(session.exportIr().body).toHaveLength(3);
    session.undo();
    session.replaceText("p1", "content", 0, 1, "Z");
    expect(session.canRedo).toBe(false);
    expect(session.undo()).toBe(true);
    expect(session.redo()).toBe(true);
    expect(session.undo() && session.undo()).toBe(false);
  });

  it("merges rapid typing in one text part into one undo step", () => {
    let now = 1000;
    const session = new DocumentEditSession(irToEditorState(makeIr([para("p1", "A")])), {
      now: () => now,
    });
    const typed = (text: string) => session.setInlines("p1", "content", [{ type: "text", text }]);
    typed("AB");
    now += 100;
    typed("ABC");
    now += 100;
    typed("ABCD");
    expect(session.undo()).toBe(true);
    expect(textOfInlines(session.exportIr().body[0]?.content)).toBe("A");
    expect(session.canUndo).toBe(false);
    session.redo();
    now += 5000;
    typed("ABCDE");
    session.undo();
    expect(textOfInlines(session.exportIr().body[0]?.content)).toBe("ABCD");
  });

  it("restores a whole saved state as one undoable step", () => {
    const original = makeIr([para("p1", "A")]);
    const session = newSession(original);
    const saved = structuredClone(session.getState());
    session.replaceText("p1", "content", 0, 1, "Changed");
    session.replaceState(saved);
    expect(textOfInlines(session.exportIr().body[0]?.content)).toBe("A");
    session.undo();
    expect(textOfInlines(session.exportIr().body[0]?.content)).toBe("Changed");
  });

  it("notifies subscribers and exposes immutable snapshots", () => {
    const session = newSession(makeIr([para("p1", "A")]));
    const listener = vi.fn();
    const unsubscribe = session.subscribe(listener);
    const before = session.getSnapshot();
    expect(session.getSnapshot()).toBe(before);
    session.replaceText("p1", "content", 0, 0, "x");
    expect(listener).toHaveBeenCalled();
    expect(session.getSnapshot()).not.toBe(before);
    expect(session.getSnapshot().revision).toBeGreaterThan(before.revision);
    unsubscribe();
    listener.mockClear();
    session.undo();
    expect(listener).not.toHaveBeenCalled();
  });
});

describe("DocumentEditSession: styles", () => {
  it("applies a named style and direct overrides without losing existing ones", () => {
    const session = newSession(styledIr());
    session.setBlockStyle("body", "p2");
    session.setBlockDirect({ align: "justify", size_pt: 13, line_height: 1.4 }, "p2");
    session.setBlockDirect(
      { font: { family: "Noto Sans" }, space_before_pt: 6, space_after_pt: 8 },
      "p2",
    );
    session.setBlockDirect({ color: "#112233", background: "#ffeeaa", weight: 700 } as never, "p2");
    const out = session.exportIr().body[1];
    expect(out).toMatchObject({
      style: "body",
      direct: {
        align: "justify",
        size_pt: 13,
        line_height: 1.4,
        font: { family: "Noto Sans" },
        space_before_pt: 6,
        space_after_pt: 8,
        color: "#112233",
        background: "#ffeeaa",
      },
    });
    session.setBlockDirect({ size_pt: undefined }, "p2");
    expect(session.exportIr().body[1]?.direct).not.toHaveProperty("size_pt");
    session.clearBlockDirect("p2");
    expect(session.exportIr().body[1]?.direct).toBeUndefined();
    session.setBlockStyle(null, "p2");
    expect(session.exportIr().body[1]).toEqual(para("p2", "Second"));
  });

  it("edits the document stylesheet and reports it as a style change", () => {
    const session = newSession(styledIr());
    session.updateParagraphStyle("body", {
      size_pt: 12,
      align: "justify",
      font: { family: "Noto Serif" },
    });
    session.updateParagraphStyle("lead", { based_on: "body", color: "#185c4c" });
    session.updateCharacterStyle("accent", { weight: 700 });
    const styles = session.exportIr().styles;
    expect(styles?.paragraph?.body).toEqual({
      size_pt: 12,
      align: "justify",
      font: { family: "Noto Serif" },
    });
    expect(styles?.paragraph?.lead).toEqual({ based_on: "body", color: "#185c4c" });
    expect(styles?.character?.accent).toEqual({ color: "#185c4c", italic: true, weight: 700 });
    expect(session.getChangeSummary().styleChanges).toContain("styles");
  });

  it("deletes a paragraph style and every reference to it", () => {
    const session = newSession(styledIr());
    session.updateParagraphStyle("lead", { based_on: "body" });
    expect(session.deleteParagraphStyle("body")).toBe(true);
    const out = session.exportIr();
    expect(out.styles?.paragraph).toEqual({ lead: {} });
    expect(out.body[0]).not.toHaveProperty("style");
    expect(session.deleteParagraphStyle("missing")).toBe(false);
  });
});

describe("DocumentEditSession: mode-switch bridge", () => {
  it("returns the page position of the selected block", () => {
    const ir = laidOut();
    const session = newSession(ir);
    expect(session.getSelectionLocation()).toBeNull();
    session.selectBlock("b");
    expect(session.getSelectionLocation()).toEqual(ir.layout?.blocks?.b);
    session.selectBlock("c");
    expect(session.getSelectionLocation()).toEqual({
      page_no: 2,
      bbox: { left: 50, top: 50, right: 300, bottom: 90 },
    });
  });

  it("falls back to the source map and returns null for blocks without a position", () => {
    const ir = makeIr([para("a", "A"), para("b", "B")], {
      source_map: { a: [{ page_no: 3, bbox: { left: 1, top: 2, right: 3, bottom: 4 } }] },
    });
    const session = newSession(ir);
    session.selectBlock("a");
    expect(session.getSelectionLocation()).toEqual(ir.source_map?.a?.[0]);
    session.selectBlock("b");
    expect(session.getSelectionLocation()).toBeNull();
  });

  it("selects the block nearest a page coordinate", () => {
    const session = newSession(laidOut());
    expect(session.selectNearestPoint(1, 100, 60)).toBe("a");
    expect(session.getSelectedBlockId()).toBe("a");
    expect(session.selectNearestPoint(1, 100, 190)).toBe("b");
    expect(session.selectNearestPoint(2, 10, 10)).toBe("c");
    expect(session.selectNearestPoint(9, 0, 0)).toBeNull();
    expect(session.getSelectedBlockId()).toBe("c");
  });

  it("ignores blocks that no longer exist", () => {
    const session = newSession(laidOut());
    session.selectBlock("b");
    session.deleteBlock("b");
    expect(session.getSelectedBlockId()).not.toBe("b");
    session.selectBlock("b");
    expect(session.getSelectedBlockId()).not.toBe("b");
    expect(session.selectNearestPoint(1, 100, 220)).toBe("a");
  });
});

describe("DocumentModeEditor: canvas", () => {
  it("renders headings, paragraphs, spanned tables, figures, math, footnotes and raw blocks", () => {
    const { container } = setup(readFixture("mixed-document.ir.json"));
    expect(screen.getByRole("region", { name: "Document editor" })).toBeTruthy();
    expect(screen.getAllByRole("heading", { level: 1 }).length).toBeGreaterThan(0);
    expect(container.querySelectorAll('[data-block-type="paragraph"]').length).toBeGreaterThan(3);
    expect(container.querySelector('[data-block-type="table"] [colspan]')).not.toBeNull();
    expect(container.querySelector('[data-block-type="table"] [rowspan]')).not.toBeNull();
    expect(container.querySelector('[data-block-type="figure"] img')?.getAttribute("src")).toMatch(
      /^data:image\/png;base64,/,
    );
    expect(container.querySelector('[data-block-type="figure"] figcaption')).not.toBeNull();
    expect(container.querySelector('[data-block-type="blockMath"] [role="math"]')).not.toBeNull();
    expect(container.querySelector('[data-block-type="blockMath"] textarea')).not.toBeNull();
    expect(container.querySelector('[data-block-type="footnote"] aside')).not.toBeNull();
    expect(container.querySelector('[data-block-type="blockquote"]')).not.toBeNull();
    const raw = container.querySelector<HTMLTextAreaElement>(
      '[data-block-type="rawBlock"] textarea',
    );
    expect(raw?.value).toContain("Keep spacing");
    expect(screen.getAllByText(/exported only to matching formats/).length).toBeGreaterThan(0);
  });

  it("renders list items, task checkboxes, captions and code blocks", () => {
    const ir = makeIr([
      {
        type: "list",
        id: "l1",
        ordered: true,
        start: 3,
        tight: true,
        items: [
          { checked: null, blocks: [para("l1a", "first")] },
          { checked: true, blocks: [para("l1b", "done")] },
        ],
      },
      { type: "code", id: "c1", lang: "ts", text: "const a = 1;" },
      { type: "page_break", id: "pb" },
    ]);
    const { container, session } = setup(ir);
    expect(container.querySelector("ol")?.getAttribute("start")).toBe("3");
    const checkbox = screen.getByRole("checkbox", { name: "Task item" }) as HTMLInputElement;
    expect(checkbox.checked).toBe(true);
    fireEvent.click(checkbox);
    expect(session.exportIr().body[0]?.items?.[1]?.checked).toBe(false);
    expect((screen.getByLabelText("Code block") as HTMLTextAreaElement).value).toBe("const a = 1;");
    expect(container.querySelector('[data-block-type="pageBreak"]')?.textContent).toBe(
      "Page break",
    );
  });

  it("edits text typed into a paragraph and leaves the other blocks identical", () => {
    const ir = styledIr();
    const { container, session } = setup(ir);
    const element = editable(container, "p2");
    firstTextNode(element).nodeValue = "Second edited";
    fireEvent.input(element);
    const out = session.exportIr();
    expect(out.body[1]).toEqual(para("p2", "Second edited"));
    expect(out.body[0]).toEqual(ir.body[0]);
    expect(out.body[2]).toEqual(ir.body[2]);
    // The browser keeps its own DOM while typing: the same node is still in place.
    expect(editable(container, "p2").textContent).toBe("Second edited");
  });

  it("re-renders a block from the model after an external change", () => {
    const { container, session } = setup(styledIr());
    act(() => {
      session.replaceText("p2", "content", 0, 6, "Replaced");
    });
    expect(editable(container, "p2").textContent).toBe("Replaced");
    act(() => {
      session.undo();
    });
    expect(editable(container, "p2").textContent).toBe("Second");
  });

  it("edits a figure caption, math TeX, code and raw block text", () => {
    const { container, session } = setup(readFixture("mixed-document.ir.json"));
    const figure = container.querySelector<HTMLElement>('[data-block-type="figure"]');
    const figureId = String(figure?.dataset.blockId);
    const caption = editable(container, figureId, "caption");
    firstTextNode(caption).nodeValue = "New caption";
    fireEvent.input(caption);
    expect(session.exportIr().body.find((block) => block.id === figureId)?.caption).toEqual([
      { type: "text", text: "New caption" },
      { type: "emph", content: [{ type: "text", text: " at dawn" }] },
    ]);
    const tex = within(
      container.querySelector<HTMLElement>('[data-block-type="blockMath"]') as HTMLElement,
    ).getByLabelText("TeX source");
    fireEvent.change(tex, { target: { value: "E=mc^2" } });
    expect((tex as HTMLTextAreaElement).value).toBe("E=mc^2");
    expect(
      session.exportIr().body.some((block) => block.type === "math" && block.tex === "E=mc^2"),
    ).toBe(true);
  });

  it("never loads external images: a remote figure shows a placeholder and no request target", () => {
    const ir = makeIr([
      {
        type: "figure",
        id: "f1",
        asset: { type: "url", href: "https://tracker.example/pixel.png" },
        caption: [{ type: "text", text: "Remote" }],
      },
      para("p1", "Text"),
    ]);
    const { container, session } = setup(ir);
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector('[data-block-type="figure"]')?.textContent).toContain(
      "Image not shown",
    );
    expect(container.innerHTML).not.toContain("tracker.example");
    // The reference itself is preserved for export.
    expect(session.exportIr().body[0]?.asset).toEqual({
      type: "url",
      href: "https://tracker.example/pixel.png",
    });
  });

  it("shows the empty state for a document without blocks", () => {
    setup(makeIr([]));
    expect(screen.getByText("This document has no content yet.")).toBeTruthy();
  });

  it("accepts plain text pasted into a paragraph and ignores markup", () => {
    const { container, session } = setup(styledIr());
    const element = editable(container, "p2");
    selectRange(element, 6, 6);
    fireEvent.paste(element, { clipboardData: { getData: () => "<b>x</b>\nline" } });
    expect(textOfInlines(session.exportIr().body[1]?.content)).toBe("Second<b>x</b> line");
  });

  it("round-trips caret offsets across inline atoms", () => {
    const original = readFixture("inline-math.ir.json");
    const { container } = setup(original);
    const target = original.body[2] as IrBlock;
    const element = editable(container, target.id);
    const length = (target.content ?? []).reduce(
      (sum, node) => sum + (node.type === "text" ? (node.text?.length ?? 0) : 1),
      0,
    );
    for (const offset of [0, 3, length - 1, length]) {
      act(() => {
        writeDomSelection(element, offset, length);
      });
      expect(readDomSelection(element)).toEqual({ from: offset, to: length });
    }
  });
});

describe("DocumentModeEditor: toolbar", () => {
  it("exposes toolbar semantics, a single roving tab stop and arrow-key navigation", () => {
    const { container, session } = setup(styledIr());
    const toolbar = screen.getByRole("toolbar", { name: "Document formatting" });
    act(() => session.selectBlock("p1"));
    const items = () => Array.from(toolbar.querySelectorAll<HTMLElement>("[data-toolbar-item]"));
    expect(items().filter((item) => item.getAttribute("tabindex") === "0")).toHaveLength(1);
    const first = items().find((item) => item.getAttribute("tabindex") === "0") as HTMLElement;
    act(() => first.focus());
    fireEvent.keyDown(first, { key: "ArrowRight" });
    expect(document.activeElement).not.toBe(first);
    expect(document.activeElement?.getAttribute("tabindex")).toBe("0");
    fireEvent.keyDown(document.activeElement as Element, { key: "End" });
    const enabled = items().filter((item) => !item.hasAttribute("disabled"));
    expect(document.activeElement).toBe(enabled[enabled.length - 1]);
    fireEvent.keyDown(document.activeElement as Element, { key: "Home" });
    expect(document.activeElement).toBe(enabled[0]);
    expect(container).toBeTruthy();
  });

  it("toggles bold on the selected range and reflects it in aria-pressed", () => {
    const { container, session } = setup(styledIr());
    selectRange(editable(container, "p1"), 6, 11);
    const bold = screen.getByRole("button", { name: "Bold" });
    expect(bold.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(bold);
    expect(session.exportIr().body[0]?.content).toEqual([
      { type: "text", text: "Hello " },
      { type: "strong", content: [{ type: "text", text: "world" }] },
    ]);
    expect(screen.getByRole("button", { name: "Bold" }).getAttribute("aria-pressed")).toBe("true");
    expect(container.querySelector("strong")?.textContent).toBe("world");
    fireEvent.click(screen.getByRole("button", { name: "Bold" }));
    expect(screen.getByRole("button", { name: "Bold" }).getAttribute("aria-pressed")).toBe("false");
  });

  it.each([
    ["Italic", "emph"],
    ["Strikethrough", "strikeout"],
    ["Subscript", "subscript"],
    ["Superscript", "superscript"],
  ])("applies %s from the toolbar", (name, irType) => {
    const { container, session } = setup(styledIr());
    selectRange(editable(container, "p2"), 0, 3);
    fireEvent.click(screen.getByRole("button", { name }));
    expect(session.exportIr().body[1]?.content?.[0]?.type).toBe(irType);
  });

  it("changes the block type from the selector", () => {
    const { container, session } = setup(styledIr());
    selectRange(editable(container, "p2"), 0);
    const select = screen.getByLabelText("Block type") as HTMLSelectElement;
    expect(select.value).toBe("paragraph");
    fireEvent.change(select, { target: { value: "heading-3" } });
    expect(session.exportIr().body[1]).toMatchObject({ id: "p2", type: "heading", level: 3 });
    expect((screen.getByLabelText("Block type") as HTMLSelectElement).value).toBe("heading-3");
    expect(container.querySelector('[role="heading"][aria-level="3"]')).not.toBeNull();
  });

  it("applies alignment, a named style, font family and size", () => {
    const { container, session } = setup(styledIr());
    selectRange(editable(container, "p2"), 0);
    fireEvent.click(screen.getByRole("button", { name: "Align right" }));
    expect(session.exportIr().body[1]?.direct).toEqual({ align: "right" });
    expect(screen.getByRole("button", { name: "Align right" }).getAttribute("aria-pressed")).toBe(
      "true",
    );
    fireEvent.change(screen.getByLabelText("Paragraph style"), { target: { value: "body" } });
    expect(session.exportIr().body[1]?.style).toBe("body");
    const size = screen.getByLabelText("Font size (pt)") as HTMLInputElement;
    fireEvent.change(size, { target: { value: "16" } });
    fireEvent.blur(size);
    expect(session.exportIr().body[1]?.direct).toMatchObject({ size_pt: 16 });
    const family = screen.getByLabelText("Font family") as HTMLInputElement;
    fireEvent.change(family, { target: { value: "Noto Sans" } });
    fireEvent.keyDown(family, { key: "Enter" });
    expect(session.exportIr().body[1]?.direct).toMatchObject({ font: { family: "Noto Sans" } });
  });

  it("applies font size to the selected text only when a range is selected", () => {
    const { container, session } = setup(styledIr());
    selectRange(editable(container, "p2"), 0, 3);
    const size = screen.getByLabelText("Font size (pt)") as HTMLInputElement;
    fireEvent.change(size, { target: { value: "20" } });
    fireEvent.blur(size);
    const out = session.exportIr().body[1];
    expect(out?.direct).toBeUndefined();
    expect(out?.content?.[0]).toMatchObject({ type: "styled", direct: { size_pt: 20 } });
  });

  it("adds, removes and edits links from the link form", () => {
    const { container, session } = setup(styledIr());
    selectRange(editable(container, "p2"), 0, 6);
    fireEvent.click(screen.getByRole("button", { name: "Link" }));
    fireEvent.change(screen.getByLabelText("Link URL"), {
      target: { value: "https://example.org" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Apply link" }));
    expect(session.exportIr().body[1]?.content?.[0]).toMatchObject({
      type: "link",
      url: "https://example.org",
    });
    fireEvent.click(screen.getByRole("button", { name: "Link" }));
    expect((screen.getByLabelText("Link URL") as HTMLInputElement).value).toBe(
      "https://example.org",
    );
    fireEvent.click(screen.getByRole("button", { name: "Remove link" }));
    expect(session.exportIr().body[1]).toEqual(para("p2", "Second"));
  });

  it("inserts a table, math block, rule and footnote", () => {
    const { container, session } = setup(styledIr());
    selectRange(editable(container, "p3"), 5);
    fireEvent.click(screen.getByRole("button", { name: "Insert table" }));
    expect(container.querySelector('[data-block-type="table"]')).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Insert math (TeX)" }));
    expect(container.querySelector('[data-block-type="blockMath"]')).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Insert horizontal rule" }));
    expect(container.textContent).toContain("———");
    selectRange(editable(container, "p3"), 5);
    fireEvent.click(screen.getByRole("button", { name: "Insert footnote" }));
    expect(container.querySelector('[data-block-type="footnote"]')).not.toBeNull();
    const types = session.exportIr().body.map((block) => block.type);
    expect(types).toEqual(expect.arrayContaining(["table", "math", "footnote"]));
  });

  it("inserts a picked image as a figure with its asset", async () => {
    const { container, session } = setup(styledIr());
    selectRange(editable(container, "p3"), 0);
    const file = new File([new Uint8Array([1, 2, 3])], "a.png", { type: "image/png" });
    fireEvent.change(screen.getByTestId("insert-image-input"), { target: { files: [file] } });
    await waitFor(() =>
      expect(container.querySelector('[data-block-type="figure"] img')).not.toBeNull(),
    );
    const out = session.exportIr();
    const figure = out.body.find((block) => block.type === "figure");
    expect(assetIdOf(figure)).toBe(
      "039058c6f2c0cb492c533b0a4d14ef77cc0f78abccced5287d84a1a2011cfb81",
    );
    expect(out.assets?.[assetIdOf(figure)]).toEqual({ bytes: "AQID", media_type: "image/png" });
  });

  it("moves, duplicates and deletes the selected block from the block actions", () => {
    const { container, session } = setup(styledIr());
    selectRange(editable(container, "p2"), 0);
    fireEvent.click(screen.getByRole("button", { name: "Move block up" }));
    expect(session.exportIr().body.map((block) => block.id)).toEqual(["p2", "p1", "p3"]);
    fireEvent.click(screen.getByRole("button", { name: "Duplicate block" }));
    expect(session.exportIr().body).toHaveLength(4);
    fireEvent.click(screen.getByRole("button", { name: "Delete block" }));
    expect(session.exportIr().body).toHaveLength(3);
    expect(document.querySelector("[aria-live=polite]")?.textContent).toBe("Block deleted");
  });

  it("undoes and redoes from the toolbar and keyboard shortcuts", () => {
    const { container, session } = setup(styledIr());
    const undo = screen.getByRole("button", { name: "Undo" }) as HTMLButtonElement;
    const redo = screen.getByRole("button", { name: "Redo" }) as HTMLButtonElement;
    expect(undo.disabled).toBe(true);
    act(() => {
      session.deleteBlock("p3");
    });
    expect(undo.disabled).toBe(false);
    fireEvent.click(undo);
    expect(session.exportIr().body).toHaveLength(3);
    expect(redo.disabled).toBe(false);
    fireEvent.click(redo);
    expect(session.exportIr().body).toHaveLength(2);
    const element = editable(container, "p1");
    fireEvent.keyDown(element, { key: "z", ctrlKey: true });
    expect(session.exportIr().body).toHaveLength(3);
    fireEvent.keyDown(element, { key: "z", ctrlKey: true, shiftKey: true });
    expect(session.exportIr().body).toHaveLength(2);
    fireEvent.keyDown(element, { key: "z", metaKey: true });
    fireEvent.keyDown(element, { key: "y", ctrlKey: true });
    expect(session.exportIr().body).toHaveLength(2);
  });

  it("uses Ctrl+B and Ctrl+I on the selected range", () => {
    const { container, session } = setup(styledIr());
    const element = editable(container, "p2");
    selectRange(element, 0, 3);
    fireEvent.keyDown(element, { key: "b", ctrlKey: true });
    expect(session.exportIr().body[1]?.content?.[0]?.type).toBe("strong");
    fireEvent.keyDown(editable(container, "p2"), { key: "i", metaKey: true });
    expect(JSON.stringify(session.exportIr().body[1]?.content)).toContain('"emph"');
  });

  it("splits on Enter, joins on Backspace and moves a block with Alt+Arrow", () => {
    const { container, session } = setup(makeIr([para("p1", "Hello world"), para("p2", "Tail")]));
    const element = editable(container, "p1");
    selectRange(element, 5);
    fireEvent.keyDown(element, { key: "Enter" });
    expect(session.exportIr().body.map((block) => textOfInlines(block.content))).toEqual([
      "Hello",
      " world",
      "Tail",
    ]);
    const created = session.getSelectedBlockId() as string;
    // Focus moved to the new block, caret at its start.
    expect(document.activeElement).toBe(editable(container, created));
    fireEvent.keyDown(editable(container, created), { key: "Backspace" });
    expect(session.exportIr().body[0]).toEqual(para("p1", "Hello world"));
    selectRange(editable(container, "p2"), 0);
    fireEvent.keyDown(editable(container, "p2"), { key: "ArrowUp", altKey: true });
    expect(session.exportIr().body.map((block) => block.id)).toEqual(["p2", "p1"]);
  });

  it("inserts a hard break on Shift+Enter and moves between blocks with arrow keys", () => {
    const { container, session } = setup(makeIr([para("p1", "ab"), para("p2", "cd")]));
    const first = editable(container, "p1");
    selectRange(first, 1);
    fireEvent.keyDown(first, { key: "Enter", shiftKey: true });
    expect(session.exportIr().body[0]?.content?.map((node) => node.type)).toEqual([
      "text",
      "line_break",
      "text",
    ]);
    const second = editable(container, "p2");
    selectRange(second, 0);
    fireEvent.keyDown(second, { key: "ArrowUp" });
    expect(document.activeElement).toBe(editable(container, "p1"));
  });

  it("accepts translated labels", () => {
    setup(styledIr(), { labels: { bold: "In đậm", undo: "Hoàn tác", toolbarLabel: "Định dạng" } });
    expect(screen.getByRole("toolbar", { name: "Định dạng" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "In đậm" })).toBeTruthy();
    expect(screen.getByRole("button", { name: "Hoàn tác" })).toBeTruthy();
  });

  it("edits an inline math atom with F2", () => {
    const original = readFixture("inline-math.ir.json");
    const target = original.body[2] as IrBlock;
    const { container, session } = setup(original);
    const element = editable(container, target.id);
    const offset = (target.content ?? []).findIndex((node) => node.type === "math");
    const before = (target.content ?? [])
      .slice(0, offset)
      .reduce((n, node) => n + (node.text?.length ?? 0), 0);
    selectRange(element, before, before + 1);
    fireEvent.keyDown(element, { key: "F2" });
    const input = screen.getByLabelText("TeX source") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "q" } });
    expect(JSON.stringify(session.exportIr().body[2]?.content)).toContain('"tex":"q"');
  });
});

describe("DocumentModeEditor: tables", () => {
  it("offers table tools for the table holding the selection", () => {
    const { container, session } = setup(readFixture("table-combined-spans.ir.json"));
    const table = container.querySelector<HTMLElement>('[data-block-type="table"]') as HTMLElement;
    expect(within(table).queryByRole("group", { name: "Table tools" })).toBeNull();
    const cellEditable = table.querySelector<HTMLElement>("[data-inline-editable]") as HTMLElement;
    act(() => cellEditable.focus());
    const tools = within(table).getByRole("group", { name: "Table tools" });
    const current = () =>
      session.exportIr().body.find((block) => block.type === "table") as IrBlock;
    // The active cell is in the header, so the row is added to the header section.
    fireEvent.click(within(tools).getByRole("button", { name: "Add row" }));
    expect(current().head).toHaveLength(2);
    fireEvent.click(within(tools).getByRole("button", { name: "Add column" }));
    expect(current().columns).toHaveLength(4);
    expectRectangular(current());
    const rowSpan = within(tools).getByLabelText("Row span") as HTMLInputElement;
    fireEvent.change(rowSpan, { target: { value: "2" } });
    expect(current().head?.[0]?.[0]).toMatchObject({ rowspan: 2, colspan: 2 });
    expectRectangular(current());
    fireEvent.click(within(tools).getByRole("button", { name: "Delete column" }));
    expect(current().columns).toHaveLength(3);
    expectRectangular(current());
    fireEvent.click(within(tools).getByRole("button", { name: "Toggle header row" }));
    expectRectangular(current());
  });

  it("says so when a table change cannot be made", () => {
    const single: IrBlock = {
      type: "table",
      id: "t1",
      columns: [{ align: "default" }],
      caption: null,
      head: [],
      body: [[{ rowspan: 1, colspan: 1, blocks: [para("tc", "only cell")] }]],
    };
    const { container } = setup(makeIr([single]));
    act(() => editable(container, "tc").focus());
    fireEvent.click(screen.getByRole("button", { name: "Delete column" }));
    expect(document.querySelector("[aria-live=polite]")?.textContent).toBe(
      defaultDocumentModeLabels.tableOperationUnavailable,
    );
    fireEvent.click(screen.getByRole("button", { name: "Delete row" }));
    expect(container.querySelector('[data-block-type="table"] td')).not.toBeNull();
  });
});

describe("StylesPanel", () => {
  const open = (ir = styledIr()) => {
    const view = setup(ir);
    fireEvent.click(screen.getByRole("button", { name: "Styles panel" }));
    return view;
  };

  it("shows a prompt until a block is selected", () => {
    open();
    expect(screen.getByText("Select a block to format it.")).toBeTruthy();
  });

  it("edits direct paragraph formatting of the selected block", () => {
    const { session } = open();
    act(() => session.selectBlock("p2"));
    const region = within(screen.getByRole("region", { name: "Selected block" }));
    fireEvent.change(region.getByLabelText("Paragraph style"), { target: { value: "body" } });
    fireEvent.change(region.getByLabelText("Font size (pt)"), { target: { value: "14" } });
    fireEvent.change(region.getByLabelText("Line height"), { target: { value: "1.5" } });
    fireEvent.change(region.getByLabelText("Alignment"), { target: { value: "justify" } });
    fireEvent.change(region.getByLabelText("Space before (pt)"), { target: { value: "4" } });
    fireEvent.change(region.getByLabelText("Space after (pt)"), { target: { value: "9" } });
    fireEvent.change(region.getByLabelText("Font family"), { target: { value: "Noto Sans" } });
    fireEvent.change(region.getAllByLabelText("Text color")[0] as HTMLElement, {
      target: { value: "#ff0000" },
    });
    fireEvent.click(region.getByLabelText("Keep with next"));
    fireEvent.click(region.getByLabelText("Page break before"));
    const out = session.exportIr().body[1];
    expect(out).toMatchObject({
      style: "body",
      direct: {
        size_pt: 14,
        line_height: 1.5,
        align: "justify",
        space_before_pt: 4,
        space_after_pt: 9,
        font: { family: "Noto Sans" },
        color: "#ff0000",
        keep_with_next: true,
        page_break_before: true,
      },
    });
    const summary = session.getChangeSummary();
    expect(summary.styleChanges).toContain("p2");
    expect(summary.modifiedBlocks).toContain("p2");
    fireEvent.click(region.getByRole("button", { name: "Reset direct formatting" }));
    expect(session.exportIr().body[1]?.direct).toBeUndefined();
  });

  it("ignores invalid colors and clears a field when it is emptied", () => {
    const { session } = open();
    act(() => session.selectBlock("p2"));
    const region = within(screen.getByRole("region", { name: "Selected block" }));
    const color = region.getAllByLabelText("Text color")[0] as HTMLInputElement;
    fireEvent.change(color, { target: { value: "red" } });
    expect(session.exportIr().body[1]).toEqual(para("p2", "Second"));
    fireEvent.change(region.getByLabelText("Font size (pt)"), { target: { value: "13" } });
    fireEvent.change(region.getByLabelText("Font size (pt)"), { target: { value: "" } });
    expect(session.exportIr().body[1]?.direct ?? {}).not.toHaveProperty("size_pt");
  });

  it("applies a named character style and direct formatting to the selected text", () => {
    const { container, session } = open();
    selectRange(editable(container, "p2"), 0, 3);
    const region = within(screen.getByRole("region", { name: "Selected text" }));
    fireEvent.change(region.getByLabelText("Character style"), { target: { value: "accent" } });
    expect(session.exportIr().body[1]?.content?.[0]).toMatchObject({
      type: "styled",
      style: "accent",
    });
    fireEvent.change(region.getByLabelText("Font size (pt)"), { target: { value: "15" } });
    expect(JSON.stringify(session.exportIr().body[1]?.content)).toContain('"size_pt":15');
  });

  it("edits, creates and deletes named styles in the document stylesheet", () => {
    const { session } = open();
    const region = within(screen.getByRole("region", { name: "Document styles" }));
    fireEvent.change(region.getByLabelText("Edit paragraph style"), { target: { value: "body" } });
    fireEvent.change(region.getAllByLabelText("Font size (pt)")[0] as HTMLElement, {
      target: { value: "12.5" },
    });
    expect(session.exportIr().styles?.paragraph?.body?.size_pt).toBe(12.5);
    expect(session.getChangeSummary().styleChanges).toContain("styles");
    fireEvent.change(region.getByLabelText("New style name"), { target: { value: "Callout" } });
    fireEvent.click(region.getByRole("button", { name: "Create style" }));
    expect(session.exportIr().styles?.paragraph).toHaveProperty("Callout");
    fireEvent.change(region.getByLabelText("Based on"), { target: { value: "body" } });
    expect(session.exportIr().styles?.paragraph?.Callout).toEqual({ based_on: "body" });
    fireEvent.click(region.getByRole("button", { name: "Delete style Callout" }));
    expect(session.exportIr().styles?.paragraph).not.toHaveProperty("Callout");
    fireEvent.change(region.getByLabelText("Edit character style"), {
      target: { value: "accent" },
    });
    fireEvent.change(region.getByLabelText("Weight"), { target: { value: "700" } });
    expect(session.exportIr().styles?.character?.accent).toMatchObject({ weight: 700 });
  });
});

describe("ExportFidelityDialog", () => {
  const renderDialog = (session: DocumentEditSession, onExport = vi.fn()) => {
    const onOpenChange = vi.fn();
    render(
      <ExportFidelityDialog
        open
        onOpenChange={onOpenChange}
        session={session}
        revision={session.getSnapshot().revision}
        labels={defaultDocumentModeLabels}
        onExport={onExport}
      />,
    );
    return { onExport, onOpenChange };
  };

  it("shows the fidelity score and what changed before exporting", () => {
    const session = newSession(styledIr());
    session.replaceText("p1", "content", 0, 5, "Howdy");
    session.deleteBlock("p2");
    session.insertBlock({ kind: "paragraph", text: "new" }, { afterId: "p3" });
    session.setBlockDirect({ align: "right" }, "p3");
    renderDialog(session);
    const summary = session.getChangeSummary();
    expect(screen.getByTestId("fidelity-score").textContent).toBe(
      `${Math.round(summary.estimatedFidelityScore * 100)}%`,
    );
    expect(screen.getByTestId("count-Added blocks").textContent).toBe(
      String(summary.addedBlocks.length),
    );
    expect(screen.getByTestId("count-Modified blocks").textContent).toBe(
      String(summary.modifiedBlocks.length),
    );
    expect(screen.getByTestId("count-Deleted blocks").textContent).toBe("1");
    expect(screen.getByTestId("count-Style changes").textContent).toBe(
      String(summary.styleChanges.length),
    );
    expect(summary.addedBlocks).toHaveLength(1);
    expect(summary.styleChanges).toContain("p3");
    const list = screen.getByRole("region", { name: "What changed" });
    expect(within(list).getByText(/Deleted blocks · p2/)).toBeTruthy();
    expect(within(list).getByText(/Modified blocks · p1/)).toBeTruthy();
  });

  it("states when nothing changed and shows a perfect score", () => {
    renderDialog(newSession(styledIr()));
    expect(screen.getByTestId("fidelity-score").textContent).toBe("100%");
    expect(screen.getByText("No changes since the document was loaded.")).toBeTruthy();
  });

  it.each([
    ["PDF", "pdf"],
    ["DOCX", "docx"],
    ["Markdown", "md"],
    ["HTML", "html"],
  ] as [string, ExportFormat][])("exports the edited IR as %s", async (label, format) => {
    const session = newSession(styledIr());
    session.replaceText("p2", "content", 0, 6, "Edited");
    const { onExport, onOpenChange } = renderDialog(session);
    fireEvent.click(screen.getByLabelText(label));
    fireEvent.click(screen.getByRole("button", { name: "Confirm export" }));
    await waitFor(() => expect(onExport).toHaveBeenCalledTimes(1));
    const request = onExport.mock.calls[0]?.[0] as ExportRequest;
    expect(request.format).toBe(format);
    expect(request.ir).toEqual(session.exportIr());
    expect(JSON.parse(request.irJson)).toEqual(request.ir);
    expect(request.summary).toEqual(summarizeEditorChanges(styledIr(), request.ir));
    expect(textOfInlines(request.ir.body[1]?.content)).toBe("Edited");
    await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
  });

  it("cancels without exporting", () => {
    const { onExport, onOpenChange } = renderDialog(newSession(styledIr()));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(onOpenChange).toHaveBeenCalledWith(false);
    expect(onExport).not.toHaveBeenCalled();
  });

  it("reports an export that fails and keeps the dialog open", async () => {
    const onExport = vi.fn().mockRejectedValue(new Error("engine offline"));
    const { onOpenChange } = renderDialog(newSession(styledIr()), onExport);
    fireEvent.click(screen.getByRole("button", { name: "Confirm export" }));
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("engine offline"));
    expect(onOpenChange).not.toHaveBeenCalledWith(false);
  });

  it("is opened from the toolbar and hands the IR to the host", async () => {
    const onExport = vi.fn();
    setup(styledIr(), { onExport, defaultExportFormat: "docx" });
    fireEvent.click(screen.getByRole("button", { name: "Export" }));
    expect((screen.getByLabelText("DOCX") as HTMLInputElement).checked).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Confirm export" }));
    await waitFor(() => expect(onExport).toHaveBeenCalled());
    expect(onExport.mock.calls[0]?.[0].format).toBe("docx");
  });

  it("refuses to export when the sections cannot be resolved", () => {
    const session = newSession(styledIr());
    vi.spyOn(session, "buildExport").mockImplementation(() => {
      throw new Error("A section start was deleted or reordered");
    });
    renderDialog(session);
    expect(screen.getByRole("alert").textContent).toContain("section start was deleted");
    expect(
      (screen.getByRole("button", { name: "Confirm export" }) as HTMLButtonElement).disabled,
    ).toBe(true);
  });
});

describe("Version history and autosave", () => {
  const openHistory = () =>
    fireEvent.click(screen.getByRole("button", { name: "Version history" }));

  it("autosaves after the debounce, then lists the autosave with kind, time and size", async () => {
    const { session, store } = setup(styledIr(), { autosaveIntervalMs: 20 });
    expect(screen.getByTestId("autosave-status").textContent).toBe("No changes yet");
    act(() => {
      session.replaceText("p1", "content", 0, 0, "A");
    });
    expect(screen.getByTestId("autosave-status").textContent).toBe("Unsaved changes");
    await waitFor(async () => expect(await store.listVersions("doc-1")).toHaveLength(1));
    await waitFor(() =>
      expect(screen.getByTestId("autosave-status").textContent).toMatch(/^Saved /),
    );
    openHistory();
    const item = await waitFor(() => {
      const found = document.querySelector<HTMLElement>("[data-version-id]");
      if (!found) throw new Error("not listed yet");
      return found;
    });
    expect(item.dataset.kind).toBe("autosave");
    expect(within(item).getByText("Autosave")).toBeTruthy();
    expect(within(item).getByText(/\d+ bytes/)).toBeTruthy();
    expect(item.querySelector("time")?.getAttribute("datetime")).toMatch(/^\d{4}-\d\d-\d\dT/);
  });

  it("debounces rapid edits into one autosave", async () => {
    const { session, store } = setup(styledIr(), { autosaveIntervalMs: 60 });
    for (const text of ["a", "b", "c"]) {
      act(() => {
        session.replaceText("p1", "content", 0, 0, text);
      });
      await new Promise((resolve) => setTimeout(resolve, 15));
    }
    await waitFor(async () => expect(await store.listVersions("doc-1")).toHaveLength(1), {
      timeout: 2000,
    });
    await new Promise((resolve) => setTimeout(resolve, 150));
    expect(await store.listVersions("doc-1")).toHaveLength(1);
  });

  it("does not autosave when it is turned off", async () => {
    const { session, store } = setup(styledIr(), { autosaveIntervalMs: 0 });
    act(() => {
      session.replaceText("p1", "content", 0, 0, "A");
    });
    expect(screen.getByTestId("autosave-status").textContent).toBe("Autosave off");
    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(await store.listVersions("doc-1")).toHaveLength(0);
  });

  it("saves a labelled manual checkpoint and restores it into the editor", async () => {
    const { container, session, store } = setup(styledIr());
    openHistory();
    fireEvent.change(screen.getByLabelText("Checkpoint label"), {
      target: { value: "Before rewrite" },
    });
    fireEvent.click(
      screen.getAllByRole("button", { name: "Save checkpoint" }).at(-1) as HTMLElement,
    );
    const item = await waitFor(() => {
      const found = document.querySelector<HTMLElement>("[data-version-id]");
      if (!found) throw new Error("not listed yet");
      return found;
    });
    expect(item.dataset.kind).toBe("manual");
    expect(within(item).getByText("Checkpoint")).toBeTruthy();
    expect(within(item).getByText("Before rewrite")).toBeTruthy();
    expect((await store.listVersions("doc-1"))[0]?.label).toBe("Before rewrite");

    act(() => {
      session.replaceText("p2", "content", 0, 6, "Rewritten");
      session.deleteBlock("p3");
    });
    expect(editable(container, "p2").textContent).toBe("Rewritten");
    fireEvent.click(within(item).getByRole("button", { name: /Restore version/ }));
    await waitFor(() => expect(editable(container, "p2").textContent).toBe("Second"));
    expect(editable(container, "p3").textContent).toBe("Third");
    expect(screen.getByText("Version restored")).toBeTruthy();
    expect(session.exportIr()).toEqual(styledIr());
    // Restoring is itself undoable.
    act(() => {
      session.undo();
    });
    expect(editable(container, "p2").textContent).toBe("Rewritten");
  });

  it("saves a checkpoint from the toolbar button", async () => {
    const { store } = setup(styledIr());
    fireEvent.click(screen.getByRole("button", { name: "Save checkpoint" }));
    await waitFor(async () => expect(await store.listVersions("doc-1")).toHaveLength(1));
    expect((await store.listVersions("doc-1"))[0]?.kind).toBe("manual");
  });

  it("shows an empty state and storage errors", async () => {
    const { store } = setup(styledIr());
    vi.spyOn(store, "listVersions").mockRejectedValue(new Error("opfs denied"));
    openHistory();
    expect(screen.getByText("No saved versions yet.")).toBeTruthy();
    fireEvent.click(
      screen.getAllByRole("button", { name: "Save checkpoint" }).at(-1) as HTMLElement,
    );
    await waitFor(() => expect(screen.getByRole("alert").textContent).toContain("opfs denied"));
  });
});

describe("Redaction purges version history", () => {
  const seeded = async (props: Partial<DocumentModeEditorProps> = {}) => {
    const ctx = setup(styledIr(), props);
    await ctx.store.saveVersion("doc-1", ctx.session.getState(), { kind: "autosave" });
    await ctx.store.saveVersion("doc-1", ctx.session.getState(), {
      kind: "manual",
      label: "Sensitive",
    });
    expect(ctx.backend.sizeBytes).toBeGreaterThan(0);
    return ctx;
  };

  it("deletes every saved version, cancels the pending autosave and says so", async () => {
    const { session, store, backend, rerender } = await seeded({ autosaveIntervalMs: 120 });
    act(() => {
      session.replaceText("p1", "content", 0, 0, "SECRET ");
    });
    expect(screen.getByTestId("autosave-status").textContent).toBe("Unsaved changes");
    const onRedactionPurge = vi.fn();
    rerender(
      <DocumentModeEditor
        docId="doc-1"
        session={session}
        store={store}
        autosaveIntervalMs={120}
        redacted
        onRedactionPurge={onRedactionPurge}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "Version history" }));
    await waitFor(() => expect(onRedactionPurge).toHaveBeenCalledWith({ purgedCount: 2 }));
    expect(await store.listVersions("doc-1")).toEqual([]);
    expect(backend.sizeBytes).toBe(0);
    const banner = await screen.findByTestId("redaction-purge-banner");
    expect(banner.textContent).toContain(
      "Redaction purged 2 saved version(s) from local OPFS history so redacted content cannot be recovered from earlier snapshots.",
    );
    expect(screen.getByText("No saved versions yet.")).toBeTruthy();
    expect(screen.getByTestId("autosave-status").textContent).toBe("History purged");
    // The autosave that was pending when redaction ran never fires.
    await new Promise((resolve) => setTimeout(resolve, 250));
    expect(await store.listVersions("doc-1")).toEqual([]);
    expect(backend.sizeBytes).toBe(0);
    // Later edits are not saved either, and manual checkpoints are refused.
    act(() => {
      session.replaceText("p1", "content", 0, 0, "again ");
    });
    await new Promise((resolve) => setTimeout(resolve, 250));
    expect(backend.sizeBytes).toBe(0);
    expect((screen.getByLabelText("Checkpoint label") as HTMLInputElement).disabled).toBe(true);
  });

  it("purges when the redaction button is used and leaves other documents alone", async () => {
    const { session, store, backend } = await seeded();
    await store.saveVersion("other-doc", session.getState(), { kind: "manual" });
    fireEvent.click(screen.getByRole("button", { name: "Version history" }));
    await waitFor(() => expect(document.querySelectorAll("[data-version-id]")).toHaveLength(2));
    fireEvent.click(screen.getByRole("button", { name: "Purge history after redaction" }));
    await waitFor(() => expect(document.querySelectorAll("[data-version-id]")).toHaveLength(0));
    expect(await store.listVersions("doc-1")).toEqual([]);
    expect(await store.listVersions("other-doc")).toHaveLength(1);
    expect(backend.sizeBytes).toBeGreaterThan(0);
    expect(screen.getByTestId("redaction-purge-banner").textContent).toContain(
      "purged 2 saved version(s)",
    );
  });

  it("purges an editor that opens already redacted before it can save anything", async () => {
    const ctx = setup(styledIr(), { autosaveIntervalMs: 10, redacted: true });
    act(() => {
      ctx.session.replaceText("p1", "content", 0, 0, "x");
    });
    await new Promise((resolve) => setTimeout(resolve, 100));
    expect(await ctx.store.listVersions("doc-1")).toEqual([]);
    expect(ctx.backend.sizeBytes).toBe(0);
  });
});

describe("Mode-switch position bridge", () => {
  it("opens at the block nearest the page point and reports its location", () => {
    const ir = laidOut();
    const onSelectionLocationChange = vi.fn();
    const { container } = setup(ir, {
      initialPoint: { pageNo: 1, x: 100, y: 210 },
      onSelectionLocationChange,
    });
    expect(container.querySelector('[data-block-id="b"]')?.hasAttribute("data-selected")).toBe(
      true,
    );
    expect(onSelectionLocationChange).toHaveBeenLastCalledWith({
      blockId: "b",
      location: ir.layout?.blocks?.b,
    });
  });

  it("reports the location of the block the user selects", () => {
    const ir = laidOut();
    const onSelectionLocationChange = vi.fn();
    const { container } = setup(ir, { onSelectionLocationChange });
    expect(onSelectionLocationChange).toHaveBeenLastCalledWith({ blockId: null, location: null });
    act(() => editable(container, "c").focus());
    expect(onSelectionLocationChange).toHaveBeenLastCalledWith({
      blockId: "c",
      location: ir.layout?.blocks?.c,
    });
  });

  it("creates its own session from ir JSON text", () => {
    render(
      <DocumentModeEditor
        docId="doc-2"
        ir={JSON.stringify(styledIr())}
        autosaveIntervalMs={0}
        store={createEditorVersionStore({ backend: new MemoryVersionBackend() })}
      />,
    );
    expect(screen.getByText("Hello world")).toBeTruthy();
  });
});

describe("Edited IR round trip", () => {
  const edited = () => {
    const figureIr = readFixture("captioned-figure.ir.json");
    const [assetId, asset] = Object.entries(figureIr.assets ?? {})[0] as [
      string,
      { bytes: string },
    ];
    expect(assetId).toBeTruthy();
    const session = newSession(styledIr());
    session.setBlockType("heading-2", "p1");
    session.replaceText("p2", "content", 0, 6, "Edited paragraph");
    session.toggleMark("bold", { range: range("p3", 0, 5) });
    session.setLink("https://example.com", "Example", range("p2", 0, 6));
    session.applyCharacterDirect({ size_pt: 14 }, range("p2", 7, 12));
    session.insertBlock({ kind: "blockquote", text: "A quotation" }, { afterId: "p3" });
    session.insertBlock({ kind: "bulletList", items: ["first item", "second item"] });
    session.insertBlock({ kind: "codeBlock", language: "ts", text: "const answer = 42;" });
    const table = String(session.insertBlock({ kind: "table", rows: 2, columns: 2, header: true }));
    const cell = session.getState().doc.content.find((node) => node.attrs?.blockId === table)
      ?.content?.[0]?.content?.[0]?.content?.[0];
    session.replaceText(String(cell?.attrs?.blockId), "content", 0, 0, "Cell heading");
    session.setTableCellSpan(table, { section: "body", row: 0, column: 0 }, { colspan: 2 });
    session.insertBlock({ kind: "mathBlock", tex: "x^2 + y^2" });
    session.insertBlock({ kind: "horizontalRule" });
    session.insertImage(
      { id: assetId, base64: asset.bytes, mediaType: "image/png" },
      "Figure caption",
    );
    session.setSelection(range("p3", 5, 5));
    session.insertFootnote("Footnote body");
    session.updateParagraphStyle("body", { size_pt: 12 });
    return session;
  };

  it("exports every new block kind with unique IDs and re-imports to the same document", () => {
    const session = edited();
    const out = session.exportIr();
    const kinds = new Set<string>();
    const ids: string[] = [];
    const visit = (blocks: IrBlock[]) => {
      for (const block of blocks) {
        kinds.add(block.type);
        ids.push(block.id);
        visit(block.blocks ?? []);
        for (const item of block.items ?? []) visit(item.blocks);
        for (const row of [...(block.head ?? []), ...(block.body ?? [])]) {
          for (const cell of row) visit(cell.blocks);
        }
      }
    };
    visit(out.body);
    expect(kinds).toEqual(
      new Set([
        "heading",
        "paragraph",
        "quote",
        "list",
        "code",
        "table",
        "math",
        "figure",
        "footnote",
      ]),
    );
    expect(new Set(ids).size).toBe(ids.length);
    // Constraints the Rust core enforces when it reads the document back.
    const coreBlockTypes = new Set([
      "heading",
      "paragraph",
      "code",
      "math",
      "quote",
      "page_break",
      "list",
      "table",
      "figure",
      "footnote",
      "raw",
    ]);
    expect([...kinds].filter((kind) => !coreBlockTypes.has(kind))).toEqual([]);
    for (const assetId of Object.keys(out.assets ?? {})) expect(assetId).toMatch(/^[0-9a-f]{64}$/);
    expectRectangular(out.body.find((block) => block.type === "table"));
    const reloaded = newSession(JSON.parse(session.serializeIr()));
    expect(reloaded.exportIr()).toEqual(out);
    expect(reloaded.getChangeSummary().addedBlocks).toEqual([]);
  });
});
