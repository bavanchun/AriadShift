import { inlinesToNodes, nodesToInlines, textOf } from "./inlines";
import { clone, equal, optionalField, remember, snapshot } from "./snapshots";
import type {
  BlockPosition,
  EditorDocumentState,
  IrBlock,
  IrDocument,
  IrFields,
  IrTableCell,
  TiptapNode,
} from "./types";

const blockTypes: Record<string, string> = {
  paragraph: "paragraph",
  heading: "heading",
  code: "codeBlock",
  math: "blockMath",
  quote: "blockquote",
  page_break: "pageBreak",
  table: "table",
  figure: "figure",
  footnote: "footnote",
  raw: "rawBlock",
  thematic_break: "horizontalRule",
};

function blockToNode(block: IrBlock, state: EditorDocumentState, path: string): TiptapNode {
  const attrs: IrFields = {
    blockId: block.id,
    styleId: block.style ?? null,
    direct: clone(block.direct ?? {}),
  };
  const node: TiptapNode = { type: blockTypes[block.type] ?? "irUnknownBlock", attrs };
  const inline = (values = block.content ?? [], suffix = "content") =>
    inlinesToNodes(values, state, `${path}/${suffix}`);
  const blocks = (values = block.blocks ?? [], suffix = "blocks") =>
    values.map((value, i) => blockToNode(value, state, `${path}/${suffix}/${i}`));
  switch (block.type) {
    case "paragraph":
    case "heading":
      if (block.type === "heading") attrs.level = block.level;
      node.content = inline();
      break;
    case "code":
    case "raw":
      if (block.type === "code") attrs.language = block.lang ?? null;
      else attrs.format = block.format;
      node.content = block.text ? [{ type: "text", text: block.text }] : [];
      break;
    case "math":
      attrs.tex = block.tex;
      attrs.display = block.display;
      break;
    case "quote":
    case "footnote":
      if (block.type === "footnote") attrs.noteId = block.note_id;
      node.content = blocks();
      break;
    case "list":
      node.type = block.ordered ? "orderedList" : "bulletList";
      attrs.start = block.start ?? null;
      attrs.tight = block.tight;
      node.content = (block.items ?? []).map((item, i) =>
        remember(state, `${path}/items/${i}`, item, {
          type: "listItem",
          attrs: { checked: item.checked },
          content: blocks(item.blocks, `items/${i}/blocks`),
        }),
      );
      break;
    case "figure":
      attrs.asset = clone(block.asset);
      node.content = [{ type: "figureCaption", content: inline(block.caption ?? [], "caption") }];
      break;
    case "table":
      attrs.columns = clone(block.columns ?? []);
      node.content = [];
      if (block.caption !== null && block.caption !== undefined) {
        node.content.push({ type: "tableCaption", content: inline(block.caption, "caption") });
      }
      for (const section of ["head", "body"] as const) {
        for (const [r, row] of (block[section] ?? []).entries()) {
          node.content.push({
            type: "tableRow",
            attrs: { section },
            content: row.map((cell, c) =>
              remember(state, `${path}/${section}/${r}/${c}`, cell, {
                type:
                  cell.header === true || (section === "head" && cell.header !== false)
                    ? "tableHeader"
                    : "tableCell",
                attrs: {
                  rowspan: cell.rowspan,
                  colspan: cell.colspan,
                  header: cell.header ?? null,
                  styleId: cell.style ?? null,
                  direct: clone(cell.direct ?? {}),
                },
                content: blocks(cell.blocks, `${section}/${r}/${c}/blocks`),
              }),
            ),
          });
        }
      }
      if (block.footnotes?.length) {
        node.content.push({
          type: "tableFootnotes",
          content: inline(block.footnotes, "footnotes"),
        });
      }
  }
  return remember(state, path, block, node);
}

function nodeToCell(node: TiptapNode, state: EditorDocumentState): IrTableCell {
  const saved = snapshot(state, node);
  const original = saved?.original;
  const attrs = node.attrs ?? {};
  const cell: IrTableCell = {
    ...clone(original),
    rowspan: Number(attrs.rowspan ?? 1),
    colspan: Number(attrs.colspan ?? 1),
    blocks: (node.content ?? []).map((child) => nodeToBlock(child, state)),
  };
  optionalField(
    cell,
    "header",
    saved && saved.node.type !== node.type
      ? node.type === "tableHeader"
      : (attrs.header ?? (original ? null : node.type === "tableHeader")),
    null,
  );
  optionalField(cell, "style", attrs.styleId, null);
  optionalField(cell, "direct", attrs.direct, {});
  return cell;
}

function nodeToBlock(node: TiptapNode, state: EditorDocumentState): IrBlock {
  const saved = snapshot(state, node);
  if (saved && equal(saved.node, node)) return clone(saved.original) as IrBlock;
  const attrs = node.attrs ?? {};
  const isList = node.type === "orderedList" || node.type === "bulletList";
  const type = isList
    ? "list"
    : Object.keys(blockTypes).find((key) => blockTypes[key] === node.type);
  if (!type && !saved) throw new Error(`Unsupported editor block: ${node.type}`);
  const block = { ...clone(saved?.original), type: type ?? saved?.original.type } as IrBlock;
  if (saved && saved.original.type !== block.type) {
    for (const field of [
      "content",
      "blocks",
      "items",
      "head",
      "body",
      "caption",
      "columns",
      "footnotes",
      "level",
      "lang",
      "text",
      "tex",
      "display",
      "ordered",
      "start",
      "tight",
      "asset",
      "note_id",
      "format",
    ]) {
      delete block[field];
    }
  }
  block.id = String(attrs.blockId);
  optionalField(block, "style", attrs.styleId, null);
  optionalField(block, "direct", attrs.direct, {});
  const children = node.content ?? [];
  switch (block.type) {
    case "paragraph":
    case "heading":
      block.content = nodesToInlines(children, state);
      if (block.type === "heading") block.level = Number(attrs.level ?? 1);
      break;
    case "code":
      block.lang = (attrs.language as string | null) ?? null;
      block.text = textOf(node);
      break;
    case "raw":
      block.format = String(attrs.format ?? "html");
      block.text = textOf(node);
      break;
    case "math":
      block.tex = String(attrs.tex ?? "");
      block.display = Boolean(attrs.display ?? true);
      break;
    case "quote":
    case "footnote":
      block.blocks = children.map((child) => nodeToBlock(child, state));
      if (block.type === "footnote") block.note_id = String(attrs.noteId ?? "");
      break;
    case "list":
      block.ordered = node.type === "orderedList";
      block.start = (attrs.start as number | null) ?? null;
      block.tight = Boolean(attrs.tight ?? false);
      block.items = children.map((item) => ({
        ...clone(snapshot(state, item)?.original),
        checked: (item.attrs?.checked as boolean | null) ?? null,
        blocks: (item.content ?? []).map((child) => nodeToBlock(child, state)),
      }));
      break;
    case "figure":
      block.asset = clone(attrs.asset) as IrBlock["asset"];
      block.caption = nodesToInlines(
        children.find((child) => child.type === "figureCaption")?.content ?? [],
        state,
      );
      break;
    case "table": {
      block.columns = clone(attrs.columns ?? []) as IrBlock["columns"];
      const caption = children.find((child) => child.type === "tableCaption");
      block.caption = caption ? nodesToInlines(caption.content ?? [], state) : null;
      for (const section of ["head", "body"] as const) {
        block[section] = children
          .filter(
            (child) => child.type === "tableRow" && (child.attrs?.section ?? "body") === section,
          )
          .map((row) => (row.content ?? []).map((cell) => nodeToCell(cell, state)));
      }
      const footnotes = children.find((child) => child.type === "tableFootnotes");
      optionalField(block, "footnotes", nodesToInlines(footnotes?.content ?? [], state), []);
      break;
    }
  }
  return block;
}

export function allBlocks(document: IrDocument): IrBlock[] {
  const result: IrBlock[] = [];
  const visit = (blocks: IrBlock[]) => {
    for (const block of blocks) {
      result.push(block);
      visit(block.blocks ?? []);
      for (const item of block.items ?? []) visit(item.blocks);
      for (const row of [...(block.head ?? []), ...(block.body ?? [])]) {
        for (const cell of row) visit(cell.blocks);
      }
    }
  };
  visit(document.body);
  visit(document.furniture ?? []);
  return result;
}

function validateIds(blocks: IrBlock[]): void {
  const ids = new Set<string>();
  for (const block of blocks) {
    if (
      typeof block.id !== "string" ||
      !/^[A-Za-z][A-Za-z0-9_.-]{0,127}$/.test(block.id) ||
      ids.has(block.id)
    ) {
      throw new Error(`Invalid or duplicate block ID: ${block.id}`);
    }
    ids.add(block.id);
  }
}

export function irToEditorState(irJson: string | IrDocument): EditorDocumentState {
  const original: IrDocument = clone(typeof irJson === "string" ? JSON.parse(irJson) : irJson);
  if (original?.version !== "ariad-ir/2" || !Array.isArray(original.body)) {
    throw new Error("The document editor requires ariad-ir/2");
  }
  validateIds(allBlocks(original));
  const state: EditorDocumentState = { original, doc: { type: "doc", content: [] }, snapshots: {} };
  state.doc.content = original.body.map((block, i) => blockToNode(block, state, `body/${i}`));
  return state;
}

/** New editor blocks receive IDs once, in the mutable editor tree, before export. */
export function editorStateToIr(state: EditorDocumentState): IrDocument {
  const reserved = new Set(allBlocks(state.original).map((block) => block.id));
  const seen = new Set(allBlocks({ ...state.original, body: [] }).map((block) => block.id));
  let nextId = 1;
  const assign = (nodes: TiptapNode[]) => {
    for (const node of nodes) {
      if (
        Object.values(blockTypes).includes(node.type) ||
        ["orderedList", "bulletList", "irUnknownBlock"].includes(node.type)
      ) {
        if (!node.attrs?.blockId || seen.has(String(node.attrs.blockId))) {
          while (reserved.has(`bEditor${nextId}`)) nextId++;
          const id = `bEditor${nextId++}`;
          node.attrs = { ...node.attrs, blockId: id };
          reserved.add(id);
        } else reserved.add(String(node.attrs.blockId));
        seen.add(String(node.attrs?.blockId));
      }
      assign(node.content ?? []);
    }
  };
  // Reserve existing editor IDs before allocating, including nodes from prior exports.
  const collect = (nodes: TiptapNode[]) => {
    for (const node of nodes) {
      if (typeof node.attrs?.blockId === "string") reserved.add(node.attrs.blockId);
      collect(node.content ?? []);
    }
  };
  collect(state.doc.content);
  assign(state.doc.content);
  const result = clone(state.original);
  result.body = state.doc.content.map((node) => nodeToBlock(node, state));
  const blocks = allBlocks(result);
  validateIds(blocks);
  const ids = new Set(blocks.map((block) => block.id));
  // Reassigning a deleted section start would silently change page styles.
  // Keep section editing explicit rather than emitting an invalid IR reference.
  let previousSectionStart = -1;
  for (const [index, section] of (result.sections ?? []).entries()) {
    const start = section.start;
    const position =
      start == null && index === 0 ? 0 : result.body.findIndex((block) => block.id === start);
    if (position < 0 || position <= previousSectionStart) {
      throw new Error("A section start was deleted or reordered; update sections before exporting");
    }
    previousSectionStart = position;
  }
  // Deleted nodes must not leave dangling v2 geometry references.
  if (result.layout?.blocks) {
    for (const id of Object.keys(result.layout.blocks))
      if (!ids.has(id)) delete result.layout.blocks[id];
  }
  if (result.source_map) {
    for (const id of Object.keys(result.source_map)) if (!ids.has(id)) delete result.source_map[id];
  }
  return result;
}

export function serializeEditorIr(state: EditorDocumentState): string {
  return JSON.stringify(editorStateToIr(state));
}

function liveBlockIds(state: EditorDocumentState): Set<string> {
  const ids = new Set(allBlocks({ ...state.original, body: [] }).map((block) => block.id));
  const visit = (nodes: TiptapNode[]) => {
    for (const node of nodes) {
      if (typeof node.attrs?.blockId === "string") ids.add(node.attrs.blockId);
      visit(node.content ?? []);
    }
  };
  visit(state.doc.content);
  return ids;
}

export function blockLocationForId(
  state: EditorDocumentState,
  blockId: string,
): BlockPosition | null {
  if (!liveBlockIds(state).has(blockId)) return null;
  const doc = state.original;
  const layout = doc.layout?.blocks;
  const sourceMap = doc.source_map;
  return clone(
    (layout && Object.hasOwn(layout, blockId) ? layout[blockId] : null) ??
      (sourceMap && Object.hasOwn(sourceMap, blockId) ? sourceMap[blockId]?.[0] : null) ??
      null,
  );
}

/** Page numbers and coordinates use the IR's one-based, top-left point system. */
export function nearestBlockAtPoint(
  state: EditorDocumentState,
  page: number,
  x: number,
  y: number,
): { blockId: string; location: BlockPosition } | null {
  if (![page, x, y].every(Number.isFinite)) return null;
  const doc = state.original;
  const ids = liveBlockIds(state);
  let nearest: { blockId: string; location: BlockPosition } | null = null;
  let best = Number.POSITIVE_INFINITY;
  let bestArea = Number.POSITIVE_INFINITY;
  const positions = new Map(Object.entries(doc.source_map ?? {}));
  for (const [id, position] of Object.entries(doc.layout?.blocks ?? {})) {
    positions.set(id, [position, ...(positions.get(id) ?? [])]);
  }
  for (const [blockId, locations] of positions) {
    if (!ids.has(blockId)) continue;
    for (const location of locations) {
      const { left, top, right, bottom } = location.bbox;
      if (location.page_no !== page || ![left, top, right, bottom].every(Number.isFinite)) continue;
      const distance =
        Math.max(left - x, 0, x - right) ** 2 + Math.max(top - y, 0, y - bottom) ** 2;
      const area = (right - left) * (bottom - top);
      if (distance < best || (distance === best && area < bestArea)) {
        best = distance;
        bestArea = area;
        nearest = { blockId, location: clone(location) };
      }
    }
  }
  return nearest;
}
