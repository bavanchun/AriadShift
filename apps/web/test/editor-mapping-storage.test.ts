import {
  closeSync,
  existsSync,
  fstatSync,
  fsyncSync,
  ftruncateSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readdirSync,
  readFileSync,
  readSync,
  rmSync,
  statSync,
  writeSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import {
  blockLocationForId,
  type EditorDocumentState,
  editorStateToIr,
  type IrBlock,
  type IrDocument,
  irToEditorState,
  nearestBlockAtPoint,
  serializeEditorIr,
  summarizeEditorChanges,
  type TiptapNode,
} from "@/editor/document/mapping";
import { createEditorVersionStore, MemoryVersionBackend } from "@/editor/storage";
import { OpfsVersionBackend } from "@/editor/storage/opfs-version-backend";
import type { VersionWorkerReply, VersionWorkerRequest } from "@/editor/storage/types";

const fixtureRoot = resolve(import.meta.dirname, "../../../fixtures/editor");
const fixtures = readdirSync(fixtureRoot).filter((name) => name.endsWith(".ir.json"));
const readFixture = (name: string): IrDocument =>
  JSON.parse(readFileSync(join(fixtureRoot, name), "utf8"));
const paragraph = (id: string, text: string): IrBlock => ({
  type: "paragraph",
  id,
  content: [{ type: "text", text }],
});

function richDocument(): IrDocument {
  const table = structuredClone(readFixture("table-combined-spans.ir.json").body[2]) as IrBlock;
  table.id = "table";
  table.caption = [{ type: "text", text: "Caption" }];
  table.footnotes = [{ type: "footnote_ref", id: "note" }];
  const cell = table.head?.[0]?.[0];
  if (cell) {
    cell.style = "survey";
    cell.direct = { background: "#aabbcc" };
    cell.extension = { shading: "preserved" };
  }
  return {
    version: "ariad-ir/2",
    meta: { title: "Tiếng Việt", authors: ["AriadShift"], extension: "metadata" },
    assets: { image: { bytes: "AAEC", media_type: "image/png", extra: true } },
    styles: readFixture("styled-nesting.ir.json").styles,
    provenance: { source: { extension: 1 } },
    custom: { future: [1, null, { preserved: true }] },
    sections: [
      {
        start: "heading",
        page_size: { width_pt: 595, height_pt: 842 },
        columns: 1,
        margins: { top_pt: 40, bottom_pt: 40, left_pt: 40, right_pt: 40 },
        header: "header",
        footer: "footer",
      },
    ],
    furniture: [paragraph("header", "Running header"), paragraph("footer", "Running footer")],
    body: [
      {
        type: "heading",
        id: "heading",
        level: 2,
        style: "lead",
        direct: {},
        content: [{ type: "text", text: "Title" }],
      },
      {
        type: "paragraph",
        id: "paragraph",
        style: "body",
        extension: { keep: true },
        content: [
          { type: "text", text: "Before ", custom: 42 },
          {
            type: "strong",
            custom: "bold",
            content: [{ type: "emph", content: [{ type: "text", text: "bold italic" }] }],
          },
          {
            type: "styled",
            style: "accent",
            direct: {
              underline: true,
              strike: false,
              color: "#123456",
              font: { family: "Noto Serif", fallback: ["serif"] },
              size_pt: 13,
              weight: 600,
              highlight: "#abcdef",
              baseline_shift: -2,
              letter_spacing: 0.5,
              small_caps: true,
            },
            content: [{ type: "text", text: "styled" }],
          },
          { type: "strikeout", content: [{ type: "text", text: "strike" }] },
          { type: "superscript", content: [{ type: "text", text: "2" }] },
          { type: "subscript", content: [{ type: "text", text: "n" }] },
          { type: "code", text: "a < b", custom: true },
          {
            type: "link",
            url: "https://example.test",
            title: "title",
            custom: true,
            content: [{ type: "text", text: "link" }],
          },
          {
            type: "image",
            target: { type: "url", href: "https://example.test/img" },
            alt: "ảnh",
            title: null,
          },
          { type: "soft_break" },
          { type: "line_break" },
          { type: "math", tex: "x^2", display: false },
          { type: "math", tex: "y^2", display: true },
          { type: "footnote_ref", id: "note" },
          { type: "raw", format: "html", text: "<span>raw</span>" },
          { type: "styled", direct: {}, content: [] },
          { type: "text", text: "" },
          { type: "future_inline", value: { untouched: true } },
        ],
      },
      {
        type: "list",
        id: "list",
        ordered: true,
        start: 3,
        tight: true,
        items: [
          {
            checked: true,
            custom: "item",
            blocks: [
              paragraph("item", "List item"),
              {
                type: "list",
                id: "nestedList",
                ordered: false,
                start: null,
                tight: false,
                items: [
                  {
                    checked: null,
                    blocks: [
                      { type: "quote", id: "quote", blocks: [paragraph("quoted", "quote")] },
                    ],
                  },
                ],
              },
            ],
          },
        ],
      },
      table,
      {
        type: "figure",
        id: "figure",
        asset: { type: "asset", id: "image" },
        caption: [{ type: "text", text: "Figure caption" }],
      },
      { type: "math", id: "math", tex: "\\sum x", display: true },
      {
        type: "footnote",
        id: "footnote",
        note_id: "note",
        blocks: [paragraph("noteBody", "Footnote definition")],
      },
      { type: "code", id: "code", lang: "ts", text: "const x = 1;" },
      { type: "raw", id: "raw", format: "tex", text: "\\opaque{}" },
      { type: "page_break", id: "pageBreak" },
      { type: "thematic_break", id: "rule" },
      { type: "future_block", id: "future", extension: { payload: true } },
    ],
    layout: {
      pages: [{ page_no: 1, width: 595, height: 842 }],
      blocks: {
        heading: { page_no: 1, bbox: { left: 10, top: 10, right: 300, bottom: 30 } },
        paragraph: { page_no: 1, bbox: { left: 10, top: 40, right: 300, bottom: 100 } },
        table: { page_no: 2, bbox: { left: 10, top: 10, right: 300, bottom: 200 } },
        header: { page_no: 1, bbox: { left: 10, top: 0, right: 300, bottom: 8 } },
      },
    },
    source_map: {
      paragraph: [{ page_no: 2, bbox: { left: 10, top: 210, right: 300, bottom: 240 } }],
    },
  };
}

function findNode(state: EditorDocumentState, id: string): TiptapNode {
  const visit = (nodes: TiptapNode[]): TiptapNode | undefined => {
    for (const node of nodes) {
      if (node.attrs?.blockId === id) return node;
      const child = visit(node.content ?? []);
      if (child) return child;
    }
  };
  const result = visit(state.doc.content);
  if (!result) throw new Error(`Missing node ${id}`);
  return result;
}

describe("IR editor mapping", () => {
  it.each(fixtures)("round-trips the golden %s, including after a neighbouring edit", (name) => {
    const ir = readFixture(name);
    const state = irToEditorState(JSON.stringify(ir));
    expect(editorStateToIr(state)).toEqual(ir);
    expect(JSON.parse(serializeEditorIr(state))).toEqual(ir);
    const first = state.doc.content[0];
    if (first?.type !== "paragraph") throw new Error("Fixture must have an editable neighbour");
    first.content = [{ type: "text", text: "Edited neighbour" }];
    const output = editorStateToIr(state);
    expect(output.body.slice(1)).toEqual(ir.body.slice(1));
    expect({ ...output, body: [] }).toEqual({ ...ir, body: [] });
    expect(summarizeEditorChanges(ir, output).modifiedBlocks).toEqual([ir.body[0]?.id]);
  });

  it("maps all block/inline shapes to editor nodes and preserves every JSON field", () => {
    const ir = richDocument();
    const state = irToEditorState(ir);
    expect(state.doc.content.map((node) => node.type)).toEqual([
      "heading",
      "paragraph",
      "orderedList",
      "table",
      "figure",
      "blockMath",
      "footnote",
      "codeBlock",
      "rawBlock",
      "pageBreak",
      "horizontalRule",
      "irUnknownBlock",
    ]);
    expect(
      findNode(state, "paragraph").content?.some((node) => node.marks?.[0]?.type === "bold"),
    ).toBe(true);
    expect(editorStateToIr(structuredClone(state))).toEqual(ir);
    expect(summarizeEditorChanges(ir, ir)).toEqual({
      addedBlocks: [],
      modifiedBlocks: [],
      deletedBlocks: [],
      styleChanges: [],
      estimatedFidelityScore: 1,
      changeItems: [],
    });
    state.original.meta.title = "Changed metadata";
    expect(ir.meta.title).toBe("Tiếng Việt");
  });

  it("updates text, headings, inline marks, local styles and table cells while retaining originals", () => {
    const ir = richDocument();
    const state = irToEditorState(ir);
    const p = findNode(state, "paragraph");
    const bold = p.content?.find((node) => node.marks?.[0]?.type === "bold");
    if (!bold || !p.attrs) throw new Error("Missing paragraph content");
    bold.text = "Edited bold italic";
    p.attrs.direct = { align: "center", size_pt: 15 };
    p.attrs.styleId = "lead";
    const h = findNode(state, "heading");
    h.attrs = { ...h.attrs, level: 3 };
    h.content = [{ type: "text", text: "New heading" }];
    const table = findNode(state, "table");
    const cell = table.content?.find((node) => node.type === "tableRow")?.content?.[0];
    if (!cell) throw new Error("Missing table cell");
    cell.attrs = { ...cell.attrs, colspan: 3, direct: { background: "#112233" } };
    const innerId = String(cell.content?.[0]?.attrs?.blockId);
    const inner = findNode(state, innerId);
    inner.content = [{ type: "text", text: "Edited cell" }];
    const result = editorStateToIr(state);
    expect(result.body[0]).toMatchObject({
      id: "heading",
      level: 3,
      content: [{ type: "text", text: "New heading" }],
    });
    expect(result.body[1]).toMatchObject({
      id: "paragraph",
      style: "lead",
      direct: { align: "center", size_pt: 15 },
      extension: { keep: true },
    });
    expect(result.body[1]?.content?.[0]).toEqual(ir.body[1]?.content?.[0]);
    expect(result.body[1]?.content?.[1]).toMatchObject({
      type: "strong",
      custom: "bold",
      content: [{ type: "emph", content: [{ type: "text", text: "Edited bold italic" }] }],
    });
    expect(result.body[1]?.content?.slice(2)).toEqual(ir.body[1]?.content?.slice(2));
    expect(result.body[3]?.head?.[0]?.[0]).toMatchObject({
      colspan: 3,
      rowspan: 1,
      header: true,
      style: "survey",
      direct: { background: "#112233" },
      extension: { shading: "preserved" },
    });
    expect(result.body.slice(4)).toEqual(ir.body.slice(4));
    expect({ ...result, body: [] }).toEqual({ ...ir, body: [] });
    const summary = summarizeEditorChanges(ir, result);
    expect(summary.modifiedBlocks).toEqual(["heading", "paragraph", "table", innerId]);
    expect(summary.addedBlocks).toEqual([]);
    expect(summary.deletedBlocks).toEqual([]);
    expect(summary.styleChanges).toEqual(["paragraph", "table"]);
    expect(
      summary.changeItems.filter((item) => item.kind === "modified").map((item) => item.blockId),
    ).toEqual(summary.modifiedBlocks);
  });

  it("restores source mark nesting after the editor sorts marks by schema rank", () => {
    const ir = richDocument();
    const state = irToEditorState(ir);
    const nodes = findNode(state, "paragraph").content ?? [];
    for (const node of nodes) node.marks?.reverse();
    expect(editorStateToIr(state)).toEqual(ir);
    const bold = nodes.find((node) => node.marks?.some((mark) => mark.type === "bold"));
    if (bold) bold.text = "Changed after mark sorting";
    expect(editorStateToIr(state).body[1]?.content?.[1]).toMatchObject({
      type: "strong",
      content: [{ type: "emph", content: [{ type: "text", text: "Changed after mark sorting" }] }],
    });
  });

  it("exports native table header toggles and column alignment changes", () => {
    const state = irToEditorState(richDocument());
    const table = findNode(state, "table");
    const cell = table.content?.find((child) => child.type === "tableRow")?.content?.[0];
    if (!cell || !table.attrs) throw new Error("Missing table");
    cell.type = "tableCell";
    const columns = structuredClone(table.attrs.columns) as { align: string }[];
    if (columns[0]) columns[0].align = "center";
    table.attrs.columns = columns;
    const result = editorStateToIr(state);
    expect(result.body[3]?.head?.[0]?.[0]?.header).toBe(false);
    expect(result.body[3]?.columns?.[0]?.align).toBe("center");
  });

  it("supports new native marks, style overrides, list edits, caption edits and new stable blocks", () => {
    const state = irToEditorState(richDocument());
    const p = findNode(state, "paragraph");
    p.content = [
      {
        type: "text",
        text: "New text",
        marks: [
          { type: "underline" },
          { type: "textStyle", attrs: { color: "#123456", fontFamily: "Noto Sans", sizePt: 12 } },
        ],
      },
    ];
    const list = findNode(state, "list");
    if (list.content?.[0]) list.content[0].attrs = { ...list.content[0].attrs, checked: false };
    findNode(state, "figure").content = [
      { type: "figureCaption", content: [{ type: "text", text: "Edited caption" }] },
    ];
    state.doc.content.push({ type: "paragraph", content: [{ type: "text", text: "Added" }] });
    const result = editorStateToIr(state);
    const newId = result.body.at(-1)?.id;
    expect(newId).toMatch(/^bEditor/);
    expect(editorStateToIr(state).body.at(-1)?.id).toBe(newId);
    expect(result.body[1]?.content).toEqual([
      {
        type: "styled",
        direct: { underline: true },
        content: [
          {
            type: "styled",
            direct: { color: "#123456", font: { family: "Noto Sans" }, size_pt: 12 },
            content: [{ type: "text", text: "New text" }],
          },
        ],
      },
    ]);
    expect(result.body[2]?.items?.[0]).toMatchObject({ checked: false, custom: "item" });
    expect(result.body[4]?.caption).toEqual([{ type: "text", text: "Edited caption" }]);
    expect(summarizeEditorChanges(state.original, result).addedBlocks).toEqual([newId]);
  });

  it("detects deletions, stylesheet and metadata changes and drops only dangling geometry", () => {
    const ir = richDocument();
    const state = irToEditorState(ir);
    state.doc.content = state.doc.content.filter((node) => node.attrs?.blockId !== "paragraph");
    state.original.styles = { ...state.original.styles, character: {} };
    state.original.meta.title = "Updated";
    const result = editorStateToIr(state);
    expect(result.layout?.blocks?.paragraph).toBeUndefined();
    expect(result.source_map?.paragraph).toBeUndefined();
    expect(result.layout?.blocks?.heading).toEqual(ir.layout?.blocks?.heading);
    const summary = summarizeEditorChanges(ir, result);
    expect(summary.deletedBlocks).toEqual(["paragraph"]);
    expect(summary.styleChanges).toEqual(["styles"]);
    expect(summary.estimatedFidelityScore).toBeLessThan(1);
    expect(summary.estimatedFidelityScore).toBeGreaterThanOrEqual(0);
    expect(summary.changeItems).toContainEqual({
      kind: "document",
      message: "Document meta changed",
    });
  });

  it("reports inline style edits and preserves extended text while changing its text", () => {
    const ir = richDocument();
    const state = irToEditorState(ir);
    const node = findNode(state, "paragraph");
    const extended = node.content?.[0];
    const styled = node.content?.find((child) => child.marks?.[0]?.type === "irStyle");
    if (!extended || !styled?.marks?.[0]?.attrs) throw new Error("Missing styled text");
    extended.text = "New extended text";
    styled.marks[0].attrs.direct = { underline: false, color: "#654321" };
    const result = editorStateToIr(state);
    expect(result.body[1]?.content?.[0]).toEqual({
      type: "text",
      text: "New extended text",
      custom: 42,
    });
    expect(result.body[1]?.content?.[2]).toMatchObject({
      type: "styled",
      style: "accent",
      direct: { underline: false, color: "#654321" },
    });
    expect(summarizeEditorChanges(ir, result).styleChanges).toEqual(["paragraph"]);
  });

  it("distinguishes nested selection rectangles and retains existing allocated IDs", () => {
    const ir = richDocument();
    const blocks = ir.layout?.blocks;
    if (!blocks) throw new Error("Missing layout");
    blocks.list = { page_no: 1, bbox: { left: 10, top: 40, right: 300, bottom: 200 } };
    const state = irToEditorState(ir);
    expect(nearestBlockAtPoint(state, 1, 20, 60)?.blockId).toBe("paragraph");
    state.doc.content.push({ type: "paragraph" });
    const first = editorStateToIr(state).body.at(-1)?.id;
    state.doc.content.unshift({ type: "paragraph" });
    const result = editorStateToIr(state);
    expect(result.body.at(-1)?.id).toBe(first);
    expect(result.body[0]?.id).not.toBe(first);
  });

  it("rejects unsupported versions, duplicate IDs and unsupported new editor nodes", () => {
    expect(() => irToEditorState('{"version":"ariad-ir/1","body":[]}')).toThrow("ariad-ir/2");
    const ir = richDocument();
    ir.body.push(paragraph("heading", "duplicate"));
    expect(() => irToEditorState(ir)).toThrow("duplicate");
    const state = irToEditorState(richDocument());
    state.doc.content.push({ type: "unsupported" });
    expect(() => editorStateToIr(state)).toThrow("Unsupported editor block");
  });

  it("assigns fresh IDs throughout a copied subtree and preserves the first occurrence", () => {
    const state = irToEditorState(richDocument());
    state.doc.content.push(structuredClone(findNode(state, "list")));
    const result = editorStateToIr(state);
    const original = result.body[2];
    const copied = result.body.at(-1);
    expect(original).toEqual(state.original.body[2]);
    expect(copied?.id).not.toBe(original?.id);
    expect(copied?.items?.[0]?.blocks[0]?.id).not.toBe(original?.items?.[0]?.blocks[0]?.id);
    expect(copied?.items?.[0]?.blocks[0]?.content).toEqual(
      original?.items?.[0]?.blocks[0]?.content,
    );
    const copiedNested = copied?.items?.[0]?.blocks[1];
    const oldNested = original?.items?.[0]?.blocks[1];
    expect(copiedNested?.id).not.toBe(oldNested?.id);
    expect(copiedNested?.items?.[0]?.blocks[0]?.blocks?.[0]?.id).not.toBe(
      oldNested?.items?.[0]?.blocks[0]?.blocks?.[0]?.id,
    );
    expect(editorStateToIr(state)).toEqual(result);
  });

  it("requires explicit section reassignment after deleting its start while keeping selection usable", () => {
    const state = irToEditorState(richDocument());
    state.doc.content = state.doc.content.filter((node) => node.attrs?.blockId !== "heading");
    expect(() => editorStateToIr(state)).toThrow("section start");
    expect(blockLocationForId(state, "heading")).toBeNull();
    expect(nearestBlockAtPoint(state, 1, 20, 60)?.blockId).toBe("paragraph");
    const section = state.original.sections?.[0];
    if (section) section.start = "paragraph";
    expect(editorStateToIr(state).sections?.[0]?.start).toBe("paragraph");
  });

  it("maps layout and multi-page source fragments, excluding deleted or unmapped blocks", () => {
    const state = irToEditorState(richDocument());
    expect(blockLocationForId(state, "paragraph")).toEqual(
      state.original.layout?.blocks?.paragraph,
    );
    const location = blockLocationForId(state, "paragraph");
    if (location) location.bbox.top = 999;
    expect(blockLocationForId(state, "paragraph")?.bbox.top).toBe(40);
    expect(nearestBlockAtPoint(state, 1, 20, 20)?.blockId).toBe("heading");
    expect(nearestBlockAtPoint(state, 1, 20, 105)?.blockId).toBe("paragraph");
    expect(nearestBlockAtPoint(state, 2, 20, 220)?.blockId).toBe("paragraph");
    expect(nearestBlockAtPoint(state, 1, 20, 3)?.blockId).toBe("header");
    expect(nearestBlockAtPoint(state, 99, 0, 0)).toBeNull();
    expect(nearestBlockAtPoint(state, 1, Number.NaN, 0)).toBeNull();
    state.doc.content = state.doc.content.filter((node) => node.attrs?.blockId !== "paragraph");
    expect(blockLocationForId(state, "paragraph")).toBeNull();
    expect(blockLocationForId(state, "absent")).toBeNull();
    expect(blockLocationForId(state, "constructor")).toBeNull();
  });
});

/** File-backed OPFS contract adapter exercises actual sync reads, writes and deletion. */
class TestDirectory {
  readonly name: string;
  constructor(readonly path: string) {
    this.name = basename(path);
  }

  async getDirectoryHandle(name: string, options?: { create?: boolean }) {
    const path = join(this.path, name);
    if (options?.create) mkdirSync(path, { recursive: true });
    if (!existsSync(path)) throw new DOMException("Not found", "NotFoundError");
    return new TestDirectory(path);
  }

  async getFileHandle(name: string, options?: { create?: boolean }) {
    const path = join(this.path, name);
    if (options?.create && !existsSync(path)) closeSync(openSync(path, "wx"));
    if (!existsSync(path)) throw new DOMException("Not found", "NotFoundError");
    return {
      createSyncAccessHandle: async () => {
        const fd = openSync(path, "r+");
        return {
          getSize: () => fstatSync(fd).size,
          read: (bytes: Uint8Array, options?: { at: number }) =>
            readSync(fd, bytes, 0, bytes.length, options?.at ?? 0),
          write: (bytes: Uint8Array, options?: { at: number }) =>
            writeSync(fd, bytes, 0, bytes.length, options?.at ?? 0),
          truncate: (size: number) => ftruncateSync(fd, size),
          flush: () => fsyncSync(fd),
          close: () => closeSync(fd),
        };
      },
    };
  }

  async removeEntry(name: string, options?: { recursive?: boolean }) {
    const path = join(this.path, name);
    if (!existsSync(path)) throw new DOMException("Not found", "NotFoundError");
    rmSync(path, { recursive: options?.recursive ?? false });
  }

  async *keys() {
    yield* readdirSync(this.path);
  }
}

function directoryBytes(path: string): number {
  return readdirSync(path).reduce((total, name) => {
    const child = join(path, name);
    return total + (statSync(child).isDirectory() ? directoryBytes(child) : statSync(child).size);
  }, 0);
}

describe("editor version history", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("falls back automatically without OPFS and isolates documents and returned snapshots", async () => {
    vi.stubGlobal("navigator", {});
    const store = createEditorVersionStore();
    const state = irToEditorState(richDocument());
    const save = store.saveVersion("doc", state);
    state.doc.content = [];
    const version = await save;
    expect(version.kind).toBe("autosave");
    expect(version.sizeBytes).toBeGreaterThan(0);
    const loaded = await store.restoreVersion("doc", version.versionId);
    expect(editorStateToIr(loaded)).toEqual(richDocument());
    loaded.doc.content = [];
    expect(
      (await store.restoreVersion("doc", version.versionId)).doc.content.length,
    ).toBeGreaterThan(0);
    expect(await store.loadVersion("other", version.versionId)).toBeNull();
    await expect(store.restoreVersion("doc", "missing")).rejects.toThrow("not found");
    await store.dispose();
    await expect(store.listVersions("doc")).rejects.toThrow("closed");
  });

  it("retains manual checkpoints, prunes oldest autosaves and returns newest first", async () => {
    const backend = new MemoryVersionBackend();
    const store = createEditorVersionStore({ backend, maxVersionsPerDoc: 3 });
    const state = irToEditorState(richDocument());
    const manual = await store.saveVersion("doc", state, {
      kind: "manual",
      label: "Before editing",
    });
    const old = await store.saveVersion("doc", state);
    const recent = await store.saveVersion("doc", state);
    const newest = await store.saveVersion("doc", state);
    expect((await store.listVersions("doc")).map((version) => version.versionId)).toEqual([
      newest.versionId,
      recent.versionId,
      manual.versionId,
    ]);
    expect(await store.loadVersion("doc", old.versionId)).toBeNull();
    expect((await store.listVersions("doc"))[2]?.label).toBe("Before editing");
    expect(editorStateToIr(await store.restoreVersion("doc", manual.versionId))).toEqual(
      richDocument(),
    );
    await store.dispose();
  });

  it("enforces the default 25-version cap and bounds all-manual histories", async () => {
    const backend = new MemoryVersionBackend();
    const store = createEditorVersionStore({ backend });
    const state = irToEditorState({ version: "ariad-ir/2", meta: {}, body: [] });
    await Promise.all(Array.from({ length: 28 }, () => store.saveVersion("doc", state)));
    expect(await store.listVersions("doc")).toHaveLength(25);
    const small = createEditorVersionStore({ backend, maxVersionsPerDoc: 1 });
    const first = await small.saveVersion("manual", state, { kind: "manual" });
    const last = await small.saveVersion("manual", state, { kind: "manual" });
    expect(await small.loadVersion("manual", first.versionId)).toBeNull();
    expect((await small.listVersions("manual"))[0]?.versionId).toBe(last.versionId);
    expect(() => createEditorVersionStore({ maxVersionsPerDoc: 0 })).toThrow("positive integer");
    await store.dispose();
    await small.dispose();
  });

  it("serializes pending saves before redaction and removes every in-memory byte", async () => {
    const backend = new MemoryVersionBackend();
    const store = createEditorVersionStore({ backend });
    const state = irToEditorState(richDocument());
    const first = store.saveVersion("doc", state);
    const second = store.saveVersion("doc", state, { kind: "manual" });
    const purge = store.purgeDocumentHistory("doc", "redaction");
    const [a, b, result] = await Promise.all([first, second, purge]);
    expect(result).toEqual({ purgedCount: 2, reason: "redaction" });
    expect(await store.listVersions("doc")).toEqual([]);
    expect(await store.loadVersion("doc", a.versionId)).toBeNull();
    expect(await store.loadVersion("doc", b.versionId)).toBeNull();
    expect(backend.sizeBytes).toBe(0);
    expect(await store.purgeDocumentHistory("doc", "user")).toEqual({
      purgedCount: 0,
      reason: "user",
    });
    await store.dispose();
  });

  it("persists OPFS envelopes across stores, prunes files and purges corrupt snapshots too", async () => {
    const path = mkdtempSync(join(tmpdir(), "ariad-editor-"));
    try {
      const backend = new OpfsVersionBackend(
        new TestDirectory(path) as unknown as FileSystemDirectoryHandle,
      );
      const store = createEditorVersionStore({ backend, maxVersionsPerDoc: 2 });
      const state = irToEditorState(richDocument());
      const old = await store.saveVersion("doc/../arbitrary-識別", state);
      const manual = await store.saveVersion("doc/../arbitrary-識別", state, { kind: "manual" });
      const latest = await store.saveVersion("doc/../arbitrary-識別", state);
      expect(await store.loadVersion("doc/../arbitrary-識別", old.versionId)).toBeNull();
      expect(directoryBytes(path)).toBeGreaterThan(0);
      const reopened = createEditorVersionStore({ backend });
      expect(
        editorStateToIr(await reopened.restoreVersion("doc/../arbitrary-識別", manual.versionId)),
      ).toEqual(richDocument());
      const base = join(path, "ariadshift-editor-versions");
      const folder = readdirSync(base)[0];
      if (!folder) throw new Error("Missing OPFS document directory");
      const corrupt = openSync(join(base, folder, "corrupt.json"), "w");
      writeSync(corrupt, "broken JSON with sensitive text");
      closeSync(corrupt);
      expect(await reopened.purgeDocumentHistory("doc/../arbitrary-識別", "redaction")).toEqual({
        purgedCount: 3,
        reason: "redaction",
      });
      expect(directoryBytes(path)).toBe(0);
      expect(readdirSync(base)).toEqual([]);
      expect(await store.listVersions("doc/../arbitrary-識別")).toEqual([]);
      expect(await store.loadVersion("doc/../arbitrary-識別", latest.versionId)).toBeNull();
      await store.dispose();
      await reopened.dispose();
    } finally {
      rmSync(path, { recursive: true, force: true });
    }
  });

  it("uses the storage worker automatically, forwards errors, and releases the worker", async () => {
    const path = mkdtempSync(join(tmpdir(), "ariad-editor-worker-"));
    const workerScope: {
      onmessage?: (event: MessageEvent<VersionWorkerRequest>) => void;
      postMessage(reply: VersionWorkerReply): void;
    } = { postMessage: () => undefined };
    let terminated = false;
    class TestWorker {
      onmessage?: (event: MessageEvent<VersionWorkerReply>) => void;
      readonly ready: Promise<unknown>;
      constructor() {
        workerScope.postMessage = (reply) =>
          this.onmessage?.(new MessageEvent("message", { data: structuredClone(reply) }));
        this.ready = import("@/editor/storage/version-worker");
      }
      postMessage(request: VersionWorkerRequest) {
        void this.ready.then(() =>
          workerScope.onmessage?.(new MessageEvent("message", { data: structuredClone(request) })),
        );
      }
      terminate() {
        terminated = true;
      }
    }
    vi.resetModules();
    vi.stubGlobal("self", workerScope);
    vi.stubGlobal("Worker", TestWorker);
    vi.stubGlobal("navigator", { storage: { getDirectory: async () => new TestDirectory(path) } });
    const store = createEditorVersionStore();
    try {
      const entry = await store.saveVersion("doc", irToEditorState(richDocument()));
      expect(directoryBytes(path)).toBeGreaterThan(0);
      expect(editorStateToIr(await store.restoreVersion("doc", entry.versionId))).toEqual(
        richDocument(),
      );
      const remove = TestDirectory.prototype.removeEntry;
      const denied = vi
        .spyOn(TestDirectory.prototype, "removeEntry")
        .mockImplementation(async function (this: TestDirectory, name, options) {
          if (options?.recursive) throw new Error("Purge permission denied");
          return remove.call(this, name, options);
        });
      await expect(store.purgeDocumentHistory("doc", "redaction")).rejects.toThrow(
        "Purge permission denied",
      );
      expect(await store.listVersions("doc")).toHaveLength(1);
      denied.mockRestore();
      expect(await store.purgeDocumentHistory("doc", "redaction")).toMatchObject({
        purgedCount: 1,
      });
      expect(directoryBytes(path)).toBe(0);
    } finally {
      await store.dispose();
      expect(terminated).toBe(true);
      rmSync(path, { recursive: true, force: true });
    }
  });

  it("does not hide a failed redaction purge or switch away from stored history", async () => {
    const backend = new MemoryVersionBackend();
    const store = createEditorVersionStore({ backend });
    await store.saveVersion("doc", irToEditorState(richDocument()));
    const purge = vi.spyOn(backend, "purge").mockRejectedValueOnce(new Error("Storage denied"));
    await expect(store.purgeDocumentHistory("doc", "redaction")).rejects.toThrow("Storage denied");
    expect(await store.listVersions("doc")).toHaveLength(1);
    expect(backend.sizeBytes).toBeGreaterThan(0);
    purge.mockRestore();
    await store.purgeDocumentHistory("doc", "redaction");
    expect(backend.sizeBytes).toBe(0);
    await store.dispose();
  });

  it("does not conceal existing OPFS history when worker startup is denied", async () => {
    let terminated = false;
    class DeniedWorker {
      onmessage?: (event: MessageEvent<VersionWorkerReply>) => void;
      postMessage(request: VersionWorkerRequest) {
        queueMicrotask(() =>
          this.onmessage?.(
            new MessageEvent("message", {
              data: { requestId: request.requestId, error: "OPFS permission denied" },
            }),
          ),
        );
      }
      terminate() {
        terminated = true;
      }
    }
    vi.stubGlobal("Worker", DeniedWorker);
    vi.stubGlobal("navigator", { storage: { getDirectory: async () => undefined } });
    const store = createEditorVersionStore();
    await expect(store.listVersions("doc")).rejects.toThrow("OPFS permission denied");
    await expect(store.purgeDocumentHistory("doc", "redaction")).rejects.toThrow(
      "OPFS permission denied",
    );
    await store.dispose();
    expect(terminated).toBe(true);
  });

  it("purges earlier OPFS snapshots even when current saves use the memory fallback", async () => {
    const path = mkdtempSync(join(tmpdir(), "ariad-editor-fallback-"));
    const directory = new TestDirectory(path);
    const backend = new OpfsVersionBackend(directory as unknown as FileSystemDirectoryHandle);
    const persisted = createEditorVersionStore({ backend });
    const state = irToEditorState(richDocument());
    try {
      await persisted.saveVersion("doc", state);
      vi.stubGlobal("Worker", undefined);
      vi.stubGlobal("navigator", { storage: { getDirectory: async () => directory } });
      const fallback = createEditorVersionStore();
      await fallback.saveVersion("doc", state);
      expect(await fallback.listVersions("doc")).toHaveLength(1);
      expect(await fallback.purgeDocumentHistory("doc", "redaction")).toEqual({
        purgedCount: 2,
        reason: "redaction",
      });
      expect(await persisted.listVersions("doc")).toEqual([]);
      expect(await fallback.listVersions("doc")).toEqual([]);
      expect(directoryBytes(path)).toBe(0);
      await fallback.dispose();
    } finally {
      await persisted.dispose();
      rmSync(path, { recursive: true, force: true });
    }
  });
});
