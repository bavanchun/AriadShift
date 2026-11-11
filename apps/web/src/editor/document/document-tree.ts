import { inlineLength, normalizeInlines, plainText, splitInlines } from "./inline-ops";
import type { AssetRef, IrBlock, IrDocument, TiptapDoc, TiptapNode } from "./mapping";
import {
  appendColumn,
  cellAt,
  gridWidth,
  insertRow,
  removeColumn,
  removeRow,
  setCellSpan,
} from "./table-grid";

/** Path of child indices from the document root, e.g. `[2, 0, 1]`. */
export type Path = number[];

/** Where a node's inline content lives: the node itself, or one of its caption children. */
export type InlinePart = "content" | "caption" | "footnotes";

const PART_NODE: Record<Exclude<InlinePart, "content">, Record<string, string>> = {
  caption: { figure: "figureCaption", table: "tableCaption" },
  footnotes: { table: "tableFootnotes" },
};

export const TEXT_BLOCKS = new Set(["paragraph", "heading"]);
export const LIST_BLOCKS = new Set(["bulletList", "orderedList"]);
const CELLS = new Set(["tableCell", "tableHeader"]);

export const isBlockNode = (node: TiptapNode): boolean => typeof node.attrs?.blockId === "string";

export function nodeAt(root: TiptapNode, path: Path): TiptapNode | undefined {
  let node: TiptapNode | undefined = root;
  for (const index of path) node = node?.content?.[index];
  return node;
}

export function updateAtPath<T extends TiptapNode>(
  root: T,
  path: Path,
  change: (node: TiptapNode) => TiptapNode,
): T {
  const [index, ...rest] = path;
  if (index === undefined) return change(root) as T;
  const content = [...(root.content ?? [])];
  const child = content[index];
  if (!child) return root;
  content[index] = updateAtPath(child, rest, change);
  return { ...root, content };
}

export function setChildren<T extends TiptapNode>(root: T, path: Path, children: TiptapNode[]): T {
  return updateAtPath(root, path, (node) => ({ ...node, content: children }));
}

export function findBlockPath(root: TiptapNode, blockId: string): Path | null {
  const search = (node: TiptapNode, path: Path): Path | null => {
    for (const [index, child] of (node.content ?? []).entries()) {
      const next = [...path, index];
      if (child.attrs?.blockId === blockId) return next;
      const found = search(child, next);
      if (found) return found;
    }
    return null;
  };
  return search(root, []);
}

export function findBlock(
  root: TiptapNode,
  blockId: string,
): { node: TiptapNode; path: Path } | null {
  const path = findBlockPath(root, blockId);
  const node = path ? nodeAt(root, path) : undefined;
  return path && node ? { node, path } : null;
}

export function walkBlocks(root: TiptapNode, visit: (node: TiptapNode, path: Path) => void): void {
  const walk = (node: TiptapNode, path: Path) => {
    for (const [index, child] of (node.content ?? []).entries()) {
      const next = [...path, index];
      if (isBlockNode(child)) visit(child, next);
      walk(child, next);
    }
  };
  walk(root, []);
}

export function collectBlockIds(root: TiptapNode): Set<string> {
  const ids = new Set<string>();
  walkBlocks(root, (node) => ids.add(String(node.attrs?.blockId)));
  return ids;
}

/** Block IDs from the outermost ancestor down to (and including) `blockId`. */
export function blockChain(root: TiptapNode, blockId: string | null): string[] {
  if (!blockId) return [];
  const path = findBlockPath(root, blockId);
  if (!path) return [];
  const chain: string[] = [];
  let node: TiptapNode = root;
  for (const index of path) {
    const child: TiptapNode | undefined = node.content?.[index];
    if (!child) break;
    if (isBlockNode(child)) chain.push(String(child.attrs?.blockId));
    node = child;
  }
  return chain;
}

/** Flat list of every block ID in reading order. */
export function orderedBlockIds(root: TiptapNode): string[] {
  const ids: string[] = [];
  walkBlocks(root, (node) => ids.push(String(node.attrs?.blockId)));
  return ids;
}

function originalIds(blocks: IrBlock[] | undefined, ids: Set<string>) {
  for (const block of blocks ?? []) {
    ids.add(block.id);
    originalIds(block.blocks, ids);
    for (const item of block.items ?? []) originalIds(item.blocks, ids);
    for (const row of [...(block.head ?? []), ...(block.body ?? [])]) {
      for (const cell of row) originalIds(cell.blocks, ids);
    }
  }
}

/** Allocates IDs that never collide with a live, original or previously issued block. */
export class IdAllocator {
  private readonly used = new Set<string>();
  private readonly notes = new Set<string>();
  private blockCounter = 1;
  private noteCounter = 1;

  constructor(original: IrDocument, doc: TiptapDoc) {
    this.reserve(original, doc);
  }

  /** Adds every ID of a (restored) state so it is never reissued. */
  reserve(original: IrDocument, doc: TiptapDoc): void {
    originalIds(original.body, this.used);
    originalIds(original.furniture, this.used);
    for (const id of collectBlockIds(doc)) this.used.add(id);
    const visit = (node: TiptapNode) => {
      const note = node.attrs?.noteId;
      if (typeof note === "string") this.notes.add(note);
      node.content?.forEach(visit);
    };
    visit(doc);
  }

  block(): string {
    while (this.used.has(`bEditor${this.blockCounter}`)) this.blockCounter++;
    const id = `bEditor${this.blockCounter++}`;
    this.used.add(id);
    return id;
  }

  note(): string {
    while (this.notes.has(`note${this.noteCounter}`)) this.noteCounter++;
    const id = `note${this.noteCounter++}`;
    this.notes.add(id);
    return id;
  }
}

/** How the IR core represents a thematic break: a paragraph of three em dashes. */
export const HORIZONTAL_RULE_TEXT = "———";

const textNodes = (text: string | undefined): TiptapNode[] =>
  text ? [{ type: "text", text }] : [];
const blockAttrs = (ids: IdAllocator, extra: Record<string, unknown> = {}) => ({
  blockId: ids.block(),
  styleId: null,
  direct: {},
  ...extra,
});

export type InsertSpec =
  | { kind: "paragraph"; text?: string }
  | { kind: "heading"; level: number; text?: string }
  | { kind: "codeBlock"; language?: string | null; text?: string }
  | { kind: "blockquote"; text?: string }
  | { kind: "bulletList" | "orderedList"; items?: string[] }
  | { kind: "table"; rows: number; columns: number; header?: boolean }
  | { kind: "figure"; asset: AssetRef; caption?: string }
  | { kind: "mathBlock"; tex?: string; display?: boolean }
  | { kind: "horizontalRule" }
  | { kind: "pageBreak" }
  | { kind: "rawBlock"; format: string; text?: string }
  | { kind: "footnote"; noteId: string; text?: string };

function paragraph(ids: IdAllocator, text?: string): TiptapNode {
  return { type: "paragraph", attrs: blockAttrs(ids), content: textNodes(text) };
}

function cell(ids: IdAllocator, header: boolean): TiptapNode {
  return {
    type: header ? "tableHeader" : "tableCell",
    attrs: { rowspan: 1, colspan: 1, styleId: null, direct: {} },
    content: [paragraph(ids)],
  };
}

export function buildBlock(spec: InsertSpec, ids: IdAllocator): TiptapNode {
  switch (spec.kind) {
    case "paragraph":
      return paragraph(ids, spec.text);
    case "heading":
      return {
        type: "heading",
        attrs: blockAttrs(ids, { level: Math.min(6, Math.max(1, Math.trunc(spec.level))) }),
        content: textNodes(spec.text),
      };
    case "codeBlock":
      return {
        type: "codeBlock",
        attrs: blockAttrs(ids, { language: spec.language ?? null }),
        content: textNodes(spec.text),
      };
    case "blockquote":
      return {
        type: "blockquote",
        attrs: blockAttrs(ids),
        content: [paragraph(ids, spec.text)],
      };
    case "bulletList":
    case "orderedList":
      return {
        type: spec.kind,
        attrs: blockAttrs(ids, { start: null, tight: true }),
        content: (spec.items?.length ? spec.items : [""]).map((text) => ({
          type: "listItem",
          attrs: { checked: null },
          content: [paragraph(ids, text)],
        })),
      };
    case "table": {
      const rows = Math.max(1, Math.trunc(spec.rows));
      const columns = Math.max(1, Math.trunc(spec.columns));
      const header = spec.header ?? true;
      return {
        type: "table",
        attrs: blockAttrs(ids, {
          columns: Array.from({ length: columns }, () => ({ align: "default" })),
        }),
        content: Array.from({ length: rows }, (_, row) => {
          const head = header && row === 0;
          return {
            type: "tableRow",
            attrs: { section: head ? "head" : "body" },
            content: Array.from({ length: columns }, () => cell(ids, head)),
          };
        }),
      };
    }
    case "figure":
      return {
        type: "figure",
        attrs: blockAttrs(ids, { asset: spec.asset }),
        content: [{ type: "figureCaption", content: textNodes(spec.caption) }],
      };
    case "mathBlock":
      return {
        type: "blockMath",
        attrs: blockAttrs(ids, { tex: spec.tex ?? "", display: spec.display ?? true }),
      };
    case "horizontalRule":
      // The IR has no rule block; the core's readers represent one as this paragraph.
      return paragraph(ids, HORIZONTAL_RULE_TEXT);
    case "pageBreak":
      return { type: "pageBreak", attrs: blockAttrs(ids) };
    case "rawBlock":
      return {
        type: "rawBlock",
        attrs: blockAttrs(ids, { format: spec.format }),
        content: textNodes(spec.text),
      };
    case "footnote":
      return {
        type: "footnote",
        attrs: blockAttrs(ids, { noteId: spec.noteId }),
        content: [paragraph(ids, spec.text)],
      };
  }
}

/** Deep copy with a fresh block ID on every block; inline marks keep their original keys. */
export function cloneWithFreshIds(node: TiptapNode, ids: IdAllocator): TiptapNode {
  const copy = structuredClone(node);
  const refresh = (value: TiptapNode) => {
    if (isBlockNode(value)) value.attrs = { ...value.attrs, blockId: ids.block() };
    value.content?.forEach(refresh);
  };
  refresh(copy);
  return copy;
}

export function inlineHostIndex(node: TiptapNode, part: InlinePart): number | null {
  if (part === "content") return TEXT_BLOCKS.has(node.type) ? -1 : null;
  const type = PART_NODE[part][node.type];
  const index = (node.content ?? []).findIndex((child) => child.type === type);
  return type && index >= 0 ? index : null;
}

export function getInlines(node: TiptapNode, part: InlinePart = "content"): TiptapNode[] | null {
  const index = inlineHostIndex(node, part);
  if (index === null) return null;
  return (index === -1 ? node : node.content?.[index])?.content ?? [];
}

export function withInlines(node: TiptapNode, part: InlinePart, content: TiptapNode[]): TiptapNode {
  const index = inlineHostIndex(node, part);
  if (index === null) return node;
  if (index === -1) return { ...node, content };
  const children = [...(node.content ?? [])];
  const host = children[index];
  if (host) children[index] = { ...host, content };
  return { ...node, content: children };
}

export type BlockTypeOption =
  | "paragraph"
  | "heading-1"
  | "heading-2"
  | "heading-3"
  | "heading-4"
  | "heading-5"
  | "heading-6"
  | "quote"
  | "code"
  | "bulletList"
  | "orderedList";

/** The type shown in the toolbar: a paragraph inside a list item reads as that list. */
export function blockTypeOption(root: TiptapNode, blockId: string | null): BlockTypeOption | null {
  const found = blockId ? findBlock(root, blockId) : null;
  if (!found) return null;
  const parent = nodeAt(root, found.path.slice(0, -1));
  const grand = nodeAt(root, found.path.slice(0, -2));
  switch (found.node.type) {
    case "paragraph":
    case "heading":
      if (parent?.type === "listItem" && grand) return grand.type as BlockTypeOption;
      if (parent?.type === "blockquote") return "quote";
      return found.node.type === "heading"
        ? (`heading-${Math.min(6, Math.max(1, Number(found.node.attrs?.level ?? 1)))}` as BlockTypeOption)
        : "paragraph";
    case "codeBlock":
      return "code";
    case "blockquote":
      return "quote";
    case "bulletList":
    case "orderedList":
      return found.node.type;
    default:
      return null;
  }
}

function leafInto(node: TiptapNode, target: "paragraph" | "heading" | "code", level: number) {
  const attrs = { ...node.attrs };
  delete attrs.level;
  delete attrs.language;
  const isCode = node.type === "codeBlock";
  if (target === "code") {
    if (isCode) return node;
    return {
      ...node,
      type: "codeBlock",
      attrs: { ...attrs, language: null },
      content: textNodes(plainText(node.content ?? [])),
    };
  }
  const inline = isCode
    ? plainText(node.content ?? [])
        .split("\n")
        .flatMap((line, index): TiptapNode[] => [
          ...(index ? [{ type: "hardBreak" }] : []),
          ...textNodes(line),
        ])
    : (node.content ?? []);
  return target === "heading"
    ? { ...node, type: "heading", attrs: { ...attrs, level }, content: inline }
    : { ...node, type: "paragraph", attrs, content: inline };
}

export interface TreeEdit {
  doc: TiptapDoc;
  /** Block to select afterwards. */
  selectId: string | null;
  /** Section starts that must follow a block whose ID changed because it moved into a wrapper. */
  remap?: Record<string, string>;
}

/** Changes the type of one block, wrapping or unwrapping containers as needed. */
export function changeBlockType(
  doc: TiptapDoc,
  blockId: string,
  option: BlockTypeOption,
  ids: IdAllocator,
): TreeEdit | null {
  const found = findBlock(doc, blockId);
  if (!found) return null;
  const { node, path } = found;
  const parentPath = path.slice(0, -1);
  const parent = nodeAt(doc, parentPath);
  const grandPath = path.slice(0, -2);
  const grand = nodeAt(doc, grandPath);
  const isLeaf = node.type === "paragraph" || node.type === "heading" || node.type === "codeBlock";
  const isList = LIST_BLOCKS.has(node.type);
  if (!isLeaf && node.type !== "blockquote" && !isList) return null;

  const textTarget =
    option === "paragraph" || option === "code" || option.startsWith("heading-")
      ? option === "code"
        ? "code"
        : option === "paragraph"
          ? "paragraph"
          : "heading"
      : null;
  const level = option.startsWith("heading-") ? Number(option.slice(8)) : 1;

  if (textTarget) {
    if (isLeaf) {
      const converted = leafInto(node, textTarget, level);
      // A lone leaf in a list or quote leaves the container when it becomes a plain paragraph.
      if (textTarget === "paragraph" || textTarget === "heading") {
        if (parent?.type === "blockquote" && parent.content?.length === 1) {
          return {
            doc: setChildren(
              doc,
              grandPath,
              replaceAt(grand?.content ?? [], parentPath.at(-1) ?? 0, [converted]),
            ),
            selectId: blockId,
            remap: { [String(parent.attrs?.blockId)]: blockId },
          };
        }
        if (
          parent?.type === "listItem" &&
          grand?.content?.length === 1 &&
          parent.content?.length === 1
        ) {
          const listPath = path.slice(0, -3);
          const listParent = nodeAt(doc, listPath);
          return {
            doc: setChildren(
              doc,
              listPath,
              replaceAt(listParent?.content ?? [], grandPath.at(-1) ?? 0, [converted]),
            ),
            selectId: blockId,
            remap: { [String(grand.attrs?.blockId)]: blockId },
          };
        }
      }
      return { doc: updateAtPath(doc, path, () => converted), selectId: blockId };
    }
    // A selected container unwraps into its blocks.
    const children =
      node.type === "blockquote"
        ? (node.content ?? [])
        : (node.content ?? []).flatMap((item) => item.content ?? []);
    if (!children.length) return null;
    const siblings = replaceAt(parent?.content ?? [], path.at(-1) ?? 0, children);
    const first = children[0];
    return {
      doc: setChildren(doc, parentPath, siblings),
      selectId: String(first?.attrs?.blockId ?? ""),
      remap: first ? { [blockId]: String(first.attrs?.blockId) } : undefined,
    };
  }

  if (option === "quote") {
    if (node.type === "blockquote" || parent?.type === "blockquote") return null;
    if (!isLeaf) return null;
    const wrapper: TiptapNode = { type: "blockquote", attrs: blockAttrs(ids), content: [node] };
    return {
      doc: updateAtPath(doc, path, () => wrapper),
      selectId: blockId,
      remap: { [blockId]: String(wrapper.attrs?.blockId) },
    };
  }

  // Bulleted and numbered lists.
  const listType = option as "bulletList" | "orderedList";
  if (isList) {
    return {
      doc: updateAtPath(doc, path, (list) => ({
        ...list,
        type: listType,
        attrs: {
          ...list.attrs,
          start: listType === "orderedList" ? (list.attrs?.start ?? 1) : null,
        },
      })),
      selectId: blockId,
    };
  }
  if (!isLeaf) return null;
  if (parent?.type === "listItem" && grand && LIST_BLOCKS.has(grand.type)) {
    return {
      doc: updateAtPath(doc, grandPath, (list) => ({
        ...list,
        type: listType,
        attrs: {
          ...list.attrs,
          start: listType === "orderedList" ? (list.attrs?.start ?? 1) : null,
        },
      })),
      selectId: blockId,
    };
  }
  const list: TiptapNode = {
    type: listType,
    attrs: blockAttrs(ids, { start: null, tight: true }),
    content: [{ type: "listItem", attrs: { checked: null }, content: [node] }],
  };
  return {
    doc: updateAtPath(doc, path, () => list),
    selectId: blockId,
    remap: { [blockId]: String(list.attrs?.blockId) },
  };
}

function replaceAt<T>(items: T[], index: number, replacement: T[]): T[] {
  return [...items.slice(0, index), ...replacement, ...items.slice(index + 1)];
}

export function insertAt<T>(items: T[], index: number, value: T): T[] {
  return [...items.slice(0, index), value, ...items.slice(index)];
}

/** Removes a block, pruning containers that would be left empty. */
export function removeBlockAt(doc: TiptapDoc, path: Path, ids: IdAllocator): TiptapDoc {
  const parentPath = path.slice(0, -1);
  const index = path.at(-1) ?? 0;
  const parent = nodeAt(doc, parentPath);
  const rest = (parent?.content ?? []).filter((_, position) => position !== index);
  if (!rest.length && parent && parentPath.length > 0) {
    if (CELLS.has(parent.type)) return setChildren(doc, parentPath, [paragraph(ids)]);
    return removeBlockAt(doc, parentPath, ids);
  }
  return setChildren(doc, parentPath, rest);
}

export function moveBlockBy(doc: TiptapDoc, path: Path, delta: -1 | 1): TiptapDoc | null {
  const parentPath = path.slice(0, -1);
  const index = path.at(-1) ?? 0;
  const siblings = nodeAt(doc, parentPath)?.content ?? [];
  const target = index + delta;
  const moving = siblings[index];
  const other = siblings[target];
  if (!moving || !other || !isBlockNode(other)) return null;
  const next = [...siblings];
  next[index] = other;
  next[target] = moving;
  return setChildren(doc, parentPath, next);
}

export interface SplitResult extends TreeEdit {
  /** Offset to place the caret at in the block that received the caret. */
  caret: number;
}

/** Splits a paragraph or heading at an inline offset (Enter). */
export function splitTextBlock(
  doc: TiptapDoc,
  blockId: string,
  offset: number,
  ids: IdAllocator,
): SplitResult | null {
  const found = findBlock(doc, blockId);
  if (!found || !TEXT_BLOCKS.has(found.node.type)) return null;
  const { node, path } = found;
  const [head, tail] = splitInlines(node.content ?? [], offset);
  const atEnd = offset >= inlineLength(node.content ?? []);
  const keepsType = node.type === "paragraph" || !atEnd;
  const attrs: Record<string, unknown> = { ...node.attrs, blockId: ids.block() };
  delete attrs.irKey;
  if (!keepsType) {
    attrs.styleId = null;
    attrs.direct = {};
    delete attrs.level;
  }
  const created: TiptapNode = {
    type: keepsType ? node.type : "paragraph",
    attrs,
    content: normalizeInlines(tail),
  };
  const first: TiptapNode = { ...node, content: normalizeInlines(head) };
  const parentPath = path.slice(0, -1);
  const parent = nodeAt(doc, parentPath);
  const index = path.at(-1) ?? 0;
  const newId = String(attrs.blockId);

  const grandPath = path.slice(0, -2);
  const grand = nodeAt(doc, grandPath);
  if (parent?.type === "listItem" && grand && index === (parent.content?.length ?? 1) - 1) {
    const checked =
      parent.attrs?.checked === null || parent.attrs?.checked === undefined ? null : false;
    const item: TiptapNode = { type: "listItem", attrs: { checked }, content: [created] };
    const itemIndex = parentPath.at(-1) ?? 0;
    const withFirst = updateAtPath(doc, path, () => first);
    return {
      doc: setChildren(withFirst, grandPath, insertAt(grand.content ?? [], itemIndex + 1, item)),
      selectId: newId,
      caret: 0,
    };
  }
  const siblings = replaceAt(parent?.content ?? [], index, [first, created]);
  return { doc: setChildren(doc, parentPath, siblings), selectId: newId, caret: 0 };
}

/** Appends a text block's inlines to the previous sibling (Backspace at the start). */
export function mergeIntoPrevious(doc: TiptapDoc, blockId: string): SplitResult | null {
  const found = findBlock(doc, blockId);
  if (!found || !TEXT_BLOCKS.has(found.node.type)) return null;
  const parentPath = found.path.slice(0, -1);
  const index = found.path.at(-1) ?? 0;
  const siblings = nodeAt(doc, parentPath)?.content ?? [];
  const previous = siblings[index - 1];
  if (!previous || !TEXT_BLOCKS.has(previous.type)) return null;
  const caret = inlineLength(previous.content ?? []);
  const merged: TiptapNode = {
    ...previous,
    content: normalizeInlines([...(previous.content ?? []), ...(found.node.content ?? [])]),
  };
  const next = [...siblings.slice(0, index - 1), merged, ...siblings.slice(index + 1)];
  return {
    doc: setChildren(doc, parentPath, next),
    selectId: String(previous.attrs?.blockId),
    caret,
  };
}

export function siblingTextBlockId(doc: TiptapDoc, blockId: string, delta: -1 | 1): string | null {
  const found = findBlock(doc, blockId);
  if (!found) return null;
  const siblings = nodeAt(doc, found.path.slice(0, -1))?.content ?? [];
  const sibling = siblings[(found.path.at(-1) ?? 0) + delta];
  return sibling && TEXT_BLOCKS.has(sibling.type) ? String(sibling.attrs?.blockId) : null;
}

/**
 * Keeps `sections` consistent with the top-level block order, as the exporter requires. A start
 * block that was deleted, or moved ahead of the previous section, keeps its boundary at the same
 * body position, so the section is not stretched or silently merged.
 */
export function repairSections(
  oldOrder: string[],
  newOrder: string[],
  sections: IrDocument["sections"],
  remap: Record<string, string> = {},
): IrDocument["sections"] {
  if (!sections?.length) return sections;
  const starts = sections.map((section) => {
    const start = typeof section.start === "string" ? section.start : null;
    return start && remap[start] ? remap[start] : start;
  });
  const explicit = new Set(
    starts.filter((id): id is string => id !== null && newOrder.includes(id)),
  );
  const result: NonNullable<IrDocument["sections"]> = [];
  let previous = -1;
  for (const [index, section] of sections.entries()) {
    const start = starts[index] ?? null;
    if (index === 0 && start === null) {
      previous = 0;
      result.push(section);
      continue;
    }
    let position = start === null ? -1 : newOrder.indexOf(start);
    if (position <= previous) {
      const oldIndex = start === null ? -1 : oldOrder.indexOf(start);
      position = Math.max(previous + 1, oldIndex);
      while (position < newOrder.length && explicit.has(newOrder[position] ?? "")) position++;
      if (position >= newOrder.length) continue;
    }
    previous = position;
    result.push({ ...section, start: newOrder[position] });
  }
  return result;
}

/** A cell by section, row and its index within that row (not its grid column). */
export interface TableCellRef {
  section: "head" | "body";
  row: number;
  column: number;
}

export function rowsOfSection(table: TiptapNode, section: "head" | "body") {
  return (table.content ?? []).filter(
    (child) => child.type === "tableRow" && (child.attrs?.section ?? "body") === section,
  );
}

/** Replaces the rows of one section, keeping caption, header rows, body rows and footnotes in order. */
function withSectionRows(
  table: TiptapNode,
  section: "head" | "body",
  rows: TiptapNode[],
): TiptapNode {
  const head = section === "head" ? rows : rowsOfSection(table, "head");
  const body = section === "body" ? rows : rowsOfSection(table, "body");
  const children = table.content ?? [];
  return {
    ...table,
    content: [
      ...children.filter((child) => child.type === "tableCaption"),
      ...head,
      ...body,
      ...children.filter((child) => child.type !== "tableCaption" && child.type !== "tableRow"),
    ],
  };
}

const allRows = (table: TiptapNode) => [
  ...rowsOfSection(table, "head"),
  ...rowsOfSection(table, "body"),
];

export function tableColumnCount(table: TiptapNode): number {
  const declared = Array.isArray(table.attrs?.columns) ? table.attrs.columns.length : 0;
  return Math.max(
    declared,
    gridWidth(rowsOfSection(table, "head")),
    gridWidth(rowsOfSection(table, "body")),
  );
}

function withColumns(table: TiptapNode, columns: unknown[]): TiptapNode {
  return { ...table, attrs: { ...table.attrs, columns } };
}

const declaredColumns = (table: TiptapNode, width: number): unknown[] => {
  const columns = Array.isArray(table.attrs?.columns) ? [...table.attrs.columns] : [];
  while (columns.length < width) columns.push({ align: "default" });
  return columns;
};

export function addTableRow(
  table: TiptapNode,
  section: "head" | "body",
  afterRow: number | null,
  ids: IdAllocator,
): TiptapNode | null {
  const rows = rowsOfSection(table, section);
  const position = afterRow === null ? rows.length : afterRow + 1;
  const next = insertRow(
    rows,
    position,
    tableColumnCount(table),
    section === "head",
    (header) => cell(ids, header),
    { type: "tableRow", attrs: { section } },
  );
  return withSectionRows(table, section, next);
}

export function deleteTableRow(
  table: TiptapNode,
  ref: Pick<TableCellRef, "section" | "row">,
): TiptapNode | null {
  if (allRows(table).length <= 1) return null;
  const next = removeRow(rowsOfSection(table, ref.section), ref.row);
  return next ? withSectionRows(table, ref.section, next) : null;
}

export function addTableColumn(table: TiptapNode, ids: IdAllocator): TiptapNode | null {
  const width = tableColumnCount(table);
  let result = table;
  for (const section of ["head", "body"] as const) {
    const rows = rowsOfSection(table, section);
    if (!rows.length) continue;
    result = withSectionRows(
      result,
      section,
      appendColumn(rows, width, section === "head", (header) => cell(ids, header)),
    );
  }
  return withColumns(result, [...declaredColumns(table, width), { align: "default" }]);
}

/** Deletes the grid column the referenced cell starts in. */
export function deleteTableColumn(table: TiptapNode, ref: TableCellRef): TiptapNode | null {
  const width = tableColumnCount(table);
  const target = cellAt(rowsOfSection(table, ref.section), ref.row, ref.column);
  if (!target || width <= 1) return null;
  let result = table;
  for (const section of ["head", "body"] as const) {
    const rows = rowsOfSection(table, section);
    if (rows.length) result = withSectionRows(result, section, removeColumn(rows, target.col));
  }
  return withColumns(
    result,
    declaredColumns(table, width).filter((_, index) => index !== target.col),
  );
}

/** Merges or splits a cell; merged-over cells are removed and released cells are recreated. */
export function setTableCellSpan(
  table: TiptapNode,
  ref: TableCellRef,
  span: { rowspan?: number; colspan?: number },
  ids: IdAllocator,
): TiptapNode | null {
  const next = setCellSpan(
    rowsOfSection(table, ref.section),
    { row: ref.row, cell: ref.column },
    span,
    tableColumnCount(table),
    (header) => cell(ids, header),
  );
  return next ? withSectionRows(table, ref.section, next) : null;
}

/** Turns the first body row into a header row, or all header rows back into body rows. */
export function toggleHeaderRow(table: TiptapNode): TiptapNode | null {
  const head = rowsOfSection(table, "head");
  const retype = (rows: TiptapNode[], section: "head" | "body", type: string) =>
    rows.map((row) => ({
      ...row,
      attrs: { ...row.attrs, section },
      content: (row.content ?? []).map((item) => ({ ...item, type })),
    }));
  if (head.length) {
    return withSectionRows(withSectionRows(table, "head", []), "body", [
      ...retype(head, "body", "tableCell"),
      ...rowsOfSection(table, "body"),
    ]);
  }
  const [first, ...rest] = rowsOfSection(table, "body");
  // A header row cannot own cells that continue into the body.
  if (!first || (first.content ?? []).some((item) => Number(item.attrs?.rowspan ?? 1) > 1)) {
    return null;
  }
  return withSectionRows(
    withSectionRows(table, "head", retype([first], "head", "tableHeader")),
    "body",
    rest,
  );
}
