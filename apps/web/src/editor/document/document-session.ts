import {
  addTableColumn,
  addTableRow,
  type BlockTypeOption,
  blockChain,
  buildBlock,
  changeBlockType,
  cloneWithFreshIds,
  deleteTableColumn,
  deleteTableRow,
  findBlock,
  getInlines,
  IdAllocator,
  type InlinePart,
  type InsertSpec,
  insertAt,
  mergeIntoPrevious,
  moveBlockBy,
  nodeAt,
  removeBlockAt,
  repairSections,
  setChildren,
  setTableCellSpan,
  siblingTextBlockId,
  splitTextBlock,
  type TableCellRef,
  TEXT_BLOCKS,
  toggleHeaderRow,
  updateAtPath,
  walkBlocks,
  withInlines,
} from "./document-tree";
import {
  applyCharacterStyleId,
  applyTextStyle,
  deepEqual,
  deleteRange,
  inlineLength,
  insertInlineNode,
  type MarkKind,
  removeLink,
  replaceRange,
  setLink,
  toggleMark,
  updateAtom,
} from "./inline-ops";
import {
  type BlockPosition,
  blockLocationForId,
  type CharacterStyle,
  type EditorChangeSummary,
  type EditorDocumentState,
  editorStateToIr,
  type IrDocument,
  nearestBlockAtPoint,
  type ParagraphStyle,
  serializeEditorIr,
  summarizeEditorChanges,
  type TiptapDoc,
  type TiptapNode,
} from "./mapping";
import { compactStyle, patchStyleWithFont } from "./style-utils";

export interface EditorSelection {
  blockId: string;
  part: InlinePart;
  from: number;
  to: number;
}

export interface SessionSnapshot {
  state: EditorDocumentState;
  /** Increments on every content change, including typing. */
  revision: number;
  selectedBlockId: string | null;
  selection: EditorSelection | null;
  /** Block IDs from the outermost ancestor of the selected block down to the block itself. */
  selectedChain: string[];
  canUndo: boolean;
  canRedo: boolean;
}

export interface ExportBundle {
  ir: IrDocument;
  irJson: string;
  summary: EditorChangeSummary;
}

interface HistoryEntry {
  doc: TiptapDoc;
  original: IrDocument;
  selectedBlockId: string | null;
  selection: EditorSelection | null;
}

export interface DocumentSessionOptions {
  /** The document as loaded; the "what changed" summary compares against it. */
  baseline?: IrDocument;
  historyLimit?: number;
  /** Typing in one text part merges into one undo step while keystrokes are this close. */
  coalesceMs?: number;
  now?: () => number;
}

interface CommitOptions {
  doc?: TiptapDoc;
  original?: IrDocument;
  /** Edits sharing a key within `coalesceMs` merge into one undo step. */
  key?: string;
  selectId?: string | null;
  selection?: EditorSelection | null;
  /** Block IDs that moved into a wrapper; section starts follow them. */
  remap?: Record<string, string>;
  /** Re-validate `sections` against the new block order. */
  structural?: boolean;
}

/** Undoable edit state over the Tiptap JSON model; every operation keeps IR identity attributes. */
export class DocumentEditSession {
  private state: EditorDocumentState;
  private readonly baseline: IrDocument;
  private readonly ids: IdAllocator;
  private readonly undoStack: HistoryEntry[] = [];
  private redoStack: HistoryEntry[] = [];
  private readonly listeners = new Set<() => void>();
  private cache: SessionSnapshot | null = null;
  private revision = 0;
  private selectedBlockId: string | null = null;
  private selection: EditorSelection | null = null;
  private lastEdit: { key: string; at: number } | null = null;
  private readonly historyLimit: number;
  private readonly coalesceMs: number;
  private readonly now: () => number;

  constructor(state: EditorDocumentState, options: DocumentSessionOptions = {}) {
    this.state = state;
    this.baseline = structuredClone(options.baseline ?? state.original);
    this.ids = new IdAllocator(state.original, state.doc);
    this.historyLimit = options.historyLimit ?? 200;
    this.coalesceMs = options.coalesceMs ?? 1000;
    this.now = options.now ?? Date.now;
  }

  // ---- observable state -------------------------------------------------------------------

  subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };

  getSnapshot = (): SessionSnapshot => {
    this.cache ??= {
      state: this.state,
      revision: this.revision,
      selectedBlockId: this.selectedBlockId,
      selection: this.selection,
      selectedChain: blockChain(this.state.doc, this.selectedBlockId),
      canUndo: this.undoStack.length > 0,
      canRedo: this.redoStack.length > 0,
    };
    return this.cache;
  };

  getState(): EditorDocumentState {
    return this.state;
  }

  get canUndo(): boolean {
    return this.undoStack.length > 0;
  }

  get canRedo(): boolean {
    return this.redoStack.length > 0;
  }

  getSelectedBlockId(): string | null {
    return this.selectedBlockId;
  }

  getSelection(): EditorSelection | null {
    return this.selection;
  }

  private notify(): void {
    this.cache = null;
    for (const listener of [...this.listeners]) listener();
  }

  private exists(blockId: string | null): blockId is string {
    return blockId !== null && findBlock(this.state.doc, blockId) !== null;
  }

  // ---- history ----------------------------------------------------------------------------

  private commit(options: CommitOptions): boolean {
    const doc = options.doc ?? this.state.doc;
    let original = options.original ?? this.state.original;
    if (options.structural || options.remap) {
      const idsOf = (nodes: TiptapNode[]) => nodes.map((node) => String(node.attrs?.blockId));
      const sections = repairSections(
        idsOf(this.state.doc.content),
        idsOf(doc.content),
        original.sections,
        options.remap,
      );
      if (!deepEqual(sections, original.sections)) original = { ...original, sections };
    }
    if (doc === this.state.doc && original === this.state.original) return false;
    const at = this.now();
    const merge =
      options.key !== undefined &&
      this.lastEdit?.key === options.key &&
      at - this.lastEdit.at < this.coalesceMs;
    if (!merge) {
      this.undoStack.push({
        doc: this.state.doc,
        original: this.state.original,
        selectedBlockId: this.selectedBlockId,
        selection: this.selection,
      });
      if (this.undoStack.length > this.historyLimit) this.undoStack.shift();
    }
    this.lastEdit = options.key === undefined ? null : { key: options.key, at };
    this.redoStack = [];
    this.state = { ...this.state, doc, original };
    this.revision++;
    if (options.selectId !== undefined) this.selectedBlockId = options.selectId;
    if (options.selection !== undefined) this.selection = options.selection;
    if (!this.exists(this.selectedBlockId)) this.selectedBlockId = null;
    if (this.selection && !this.exists(this.selection.blockId)) this.selection = null;
    this.notify();
    return true;
  }

  private restore(entry: HistoryEntry): void {
    this.state = { ...this.state, doc: entry.doc, original: entry.original };
    this.selectedBlockId = this.exists(entry.selectedBlockId) ? entry.selectedBlockId : null;
    this.selection = this.exists(entry.selection?.blockId ?? null) ? entry.selection : null;
    this.lastEdit = null;
    this.revision++;
  }

  private current(): HistoryEntry {
    return {
      doc: this.state.doc,
      original: this.state.original,
      selectedBlockId: this.selectedBlockId,
      selection: this.selection,
    };
  }

  undo(): boolean {
    const entry = this.undoStack.pop();
    if (!entry) return false;
    this.redoStack.push(this.current());
    this.restore(entry);
    this.notify();
    return true;
  }

  redo(): boolean {
    const entry = this.redoStack.pop();
    if (!entry) return false;
    this.undoStack.push(this.current());
    this.restore(entry);
    this.notify();
    return true;
  }

  /** Loads another state (for example a restored version) as one undoable step. */
  replaceState(next: EditorDocumentState): void {
    this.undoStack.push(this.current());
    this.redoStack = [];
    this.ids.reserve(next.original, next.doc);
    this.state = structuredClone(next);
    this.lastEdit = null;
    this.revision++;
    if (!this.exists(this.selectedBlockId)) this.selectedBlockId = null;
    if (this.selection && !this.exists(this.selection.blockId)) this.selection = null;
    this.notify();
  }

  // ---- selection and mode-switch bridge ----------------------------------------------------

  selectBlock(blockId: string | null): void {
    const next = this.exists(blockId) ? blockId : null;
    const selection = this.selection?.blockId === next ? this.selection : null;
    if (next === this.selectedBlockId && selection === this.selection) return;
    this.selectedBlockId = next;
    this.selection = selection;
    this.notify();
  }

  setSelection(selection: EditorSelection | null): void {
    if (selection && !this.exists(selection.blockId)) return;
    const blockId = selection?.blockId ?? this.selectedBlockId;
    if (deepEqual(selection, this.selection) && blockId === this.selectedBlockId) return;
    this.selection = selection;
    this.selectedBlockId = blockId;
    this.notify();
  }

  /** Page position of the selected block, so another mode can scroll to the same place. */
  getSelectionLocation(): BlockPosition | null {
    return this.selectedBlockId ? blockLocationForId(this.state, this.selectedBlockId) : null;
  }

  /** Selects the block closest to a page coordinate (one-based page, top-left points). */
  selectNearestPoint(pageNo: number, xPt: number, yPt: number): string | null {
    const hit = nearestBlockAtPoint(this.state, pageNo, xPt, yPt);
    if (!hit) return null;
    this.selectBlock(hit.blockId);
    return hit.blockId;
  }

  // ---- text and inline marks -----------------------------------------------------------------

  private editInlines(
    blockId: string,
    part: InlinePart,
    change: (nodes: TiptapNode[]) => TiptapNode[],
    options: Pick<CommitOptions, "key" | "selection"> = {},
  ): boolean {
    const found = findBlock(this.state.doc, blockId);
    const nodes = found ? getInlines(found.node, part) : null;
    if (!found || !nodes) return false;
    const next = change(nodes);
    if (next === nodes) return false;
    const doc = updateAtPath(this.state.doc, found.path, (node) => withInlines(node, part, next));
    return this.commit({ doc, ...options });
  }

  /** Stores the inline content read back from the browser while the user types. */
  setInlines(
    blockId: string,
    part: InlinePart,
    nodes: TiptapNode[],
    options: { key?: string } = {},
  ): boolean {
    return this.editInlines(blockId, part, () => nodes, {
      key: options.key ?? `type:${blockId}:${part}`,
    });
  }

  replaceText(blockId: string, part: InlinePart, from: number, to: number, text: string): boolean {
    const caret = Math.min(from, to) + text.length;
    return this.editInlines(blockId, part, (nodes) => replaceRange(nodes, from, to, text), {
      selection: { blockId, part, from: caret, to: caret },
    });
  }

  deleteText(blockId: string, part: InlinePart, from: number, to: number): boolean {
    const caret = Math.min(from, to);
    return this.editInlines(blockId, part, (nodes) => deleteRange(nodes, from, to), {
      selection: { blockId, part, from: caret, to: caret },
    });
  }

  /** Inserts an atom at `offset`, replacing the range up to `to` when it is given. */
  insertInline(
    blockId: string,
    part: InlinePart,
    offset: number,
    node: TiptapNode,
    to = offset,
  ): boolean {
    const caret = offset + (node.type === "irEmpty" ? 0 : 1);
    return this.editInlines(
      blockId,
      part,
      (nodes) => insertInlineNode(deleteRange(nodes, offset, to), offset, node),
      { selection: { blockId, part, from: caret, to: caret } },
    );
  }

  /** Edits attributes of an atom (inline math TeX, raw inline text, footnote reference). */
  updateInlineAtom(
    blockId: string,
    part: InlinePart,
    offset: number,
    attrs: Record<string, unknown>,
  ): boolean {
    return this.editInlines(blockId, part, (nodes) => updateAtom(nodes, offset, attrs), {
      key: `atom:${blockId}:${part}:${offset}`,
    });
  }

  /** The range an inline format applies to: the selection, or the whole block for a caret. */
  private formatTarget(range?: EditorSelection): EditorSelection | null {
    const base =
      range ??
      this.selection ??
      (this.selectedBlockId
        ? { blockId: this.selectedBlockId, part: "content" as const, from: 0, to: 0 }
        : null);
    if (!base) return null;
    const found = findBlock(this.state.doc, base.blockId);
    const nodes = found ? getInlines(found.node, base.part) : null;
    if (!nodes) return null;
    return base.from === base.to ? { ...base, from: 0, to: inlineLength(nodes) } : base;
  }

  private format(
    range: EditorSelection | undefined,
    change: (nodes: TiptapNode[], target: EditorSelection) => TiptapNode[],
  ): boolean {
    const target = this.formatTarget(range);
    if (!target) return false;
    return this.editInlines(target.blockId, target.part, (nodes) => change(nodes, target), {
      selection: range ? undefined : (this.selection ?? undefined),
    });
  }

  toggleMark(
    kind: MarkKind,
    options: {
      range?: EditorSelection;
      attrs?: Record<string, unknown>;
      force?: "add" | "remove";
    } = {},
  ): boolean {
    return this.format(options.range, (nodes, target) =>
      toggleMark(nodes, target.from, target.to, kind, {
        attrs: options.attrs,
        force: options.force,
        styles: this.state.original.styles,
      }),
    );
  }

  setLink(href: string, title?: string | null, range?: EditorSelection): boolean {
    if (!href.trim()) return this.removeLink(range);
    return this.format(range, (nodes, t) =>
      setLink(nodes, t.from, t.to, { href: href.trim(), title }),
    );
  }

  removeLink(range?: EditorSelection): boolean {
    return this.format(range, (nodes, t) => removeLink(nodes, t.from, t.to));
  }

  /** Direct character formatting (font, size, colour, ...). Keys set to `undefined` clear. */
  applyCharacterDirect(patch: CharacterStyle, range?: EditorSelection): boolean {
    return this.format(range, (nodes, t) => applyTextStyle(nodes, t.from, t.to, patch));
  }

  applyCharacterStyle(styleId: string | null, range?: EditorSelection): boolean {
    return this.format(range, (nodes, t) => applyCharacterStyleId(nodes, t.from, t.to, styleId));
  }

  // ---- block text, math and code -----------------------------------------------------------

  /** Replaces the plain text of a code or raw block. */
  setBlockText(blockId: string, text: string): boolean {
    const found = findBlock(this.state.doc, blockId);
    if (!found || (found.node.type !== "codeBlock" && found.node.type !== "rawBlock")) return false;
    const doc = updateAtPath(this.state.doc, found.path, (node) => ({
      ...node,
      content: text ? [{ type: "text", text }] : [],
    }));
    return this.commit({ doc, key: `text:${blockId}` });
  }

  setBlockAttrs(blockId: string, attrs: Record<string, unknown>, key?: string): boolean {
    const found = findBlock(this.state.doc, blockId);
    if (!found) return false;
    const doc = updateAtPath(this.state.doc, found.path, (node) => ({
      ...node,
      attrs: { ...node.attrs, ...attrs },
    }));
    return this.commit({ doc, key: key ?? `attrs:${blockId}:${Object.keys(attrs).join(",")}` });
  }

  /** TeX source of a math block. */
  setMathSource(blockId: string, tex: string, display?: boolean): boolean {
    const found = findBlock(this.state.doc, blockId);
    if (found?.node.type !== "blockMath") return false;
    return this.setBlockAttrs(
      blockId,
      display === undefined ? { tex } : { tex, display },
      `math:${blockId}`,
    );
  }

  setListItemChecked(listId: string, itemIndex: number, checked: boolean | null): boolean {
    const found = findBlock(this.state.doc, listId);
    if (!found?.node.content?.[itemIndex]) return false;
    const doc = updateAtPath(this.state.doc, [...found.path, itemIndex], (item) => ({
      ...item,
      attrs: { ...item.attrs, checked },
    }));
    return this.commit({ doc });
  }

  // ---- block structure ------------------------------------------------------------------------

  private target(blockId?: string): string | null {
    return blockId ?? this.selectedBlockId;
  }

  setBlockType(option: BlockTypeOption, blockId?: string): boolean {
    const id = this.target(blockId);
    const edit = id ? changeBlockType(this.state.doc, id, option, this.ids) : null;
    if (!edit) return false;
    return this.commit({
      doc: edit.doc,
      remap: edit.remap ?? {},
      structural: true,
      selectId: edit.selectId,
      selection: null,
    });
  }

  /** Puts a new block after `anchorId` (default: the selected block) or at the document end. */
  private place(block: TiptapNode, anchorId: string | null, original?: IrDocument): string | null {
    const anchor = anchorId ? findBlock(this.state.doc, anchorId) : null;
    const parentPath = anchor ? anchor.path.slice(0, -1) : [];
    const index = anchor ? (anchor.path.at(-1) ?? 0) + 1 : this.state.doc.content.length;
    const siblings = nodeAt(this.state.doc, parentPath)?.content ?? [];
    const doc = setChildren(this.state.doc, parentPath, insertAt(siblings, index, block));
    const id = String(block.attrs?.blockId);
    return this.commit({ doc, original, structural: true, selectId: id, selection: null })
      ? id
      : null;
  }

  /** Inserts after `afterId`, after the selected block, or at the end of the document. */
  insertBlock(spec: InsertSpec, options: { afterId?: string | null } = {}): string | null {
    const anchorId = options.afterId === undefined ? this.selectedBlockId : options.afterId;
    return this.place(buildBlock(spec, this.ids), anchorId);
  }

  /**
   * Registers image bytes as an asset and inserts a figure that references it. The asset ID must
   * be the content hash of the bytes (see `contentAssetId`); an existing asset with that ID is
   * reused.
   */
  insertImage(
    asset: { id: string; base64: string; mediaType: string },
    caption?: string,
  ): string | null {
    const assets = this.state.original.assets;
    const original: IrDocument = {
      ...this.state.original,
      assets: {
        ...assets,
        [asset.id]: assets?.[asset.id] ?? { bytes: asset.base64, media_type: asset.mediaType },
      },
    };
    const spec: InsertSpec = { kind: "figure", asset: { type: "asset", id: asset.id }, caption };
    return this.place(buildBlock(spec, this.ids), this.selectedBlockId, original);
  }

  /** Inserts a footnote reference at the caret and appends the footnote block to the body. */
  insertFootnote(text = ""): { noteId: string; blockId: string } | null {
    const blockId = this.selection?.blockId ?? this.selectedBlockId;
    const found = blockId ? findBlock(this.state.doc, blockId) : null;
    if (!found || !TEXT_BLOCKS.has(found.node.type)) return null;
    const noteId = this.ids.note();
    const offset =
      this.selection?.blockId === blockId
        ? this.selection.to
        : inlineLength(found.node.content ?? []);
    const ref: TiptapNode = { type: "footnoteRef", attrs: { noteId } };
    const withRef = updateAtPath(this.state.doc, found.path, (node) =>
      withInlines(node, "content", insertInlineNode(node.content ?? [], offset, ref)),
    );
    const footnote = buildBlock({ kind: "footnote", noteId, text }, this.ids);
    const doc = { ...withRef, content: [...withRef.content, footnote] };
    const id = String(footnote.attrs?.blockId);
    return this.commit({ doc, structural: true, selectId: id, selection: null })
      ? { noteId, blockId: id }
      : null;
  }

  deleteBlock(blockId?: string): boolean {
    const id = this.target(blockId);
    const found = id ? findBlock(this.state.doc, id) : null;
    if (!found) return false;
    const parentPath = found.path.slice(0, -1);
    const siblings = nodeAt(this.state.doc, parentPath)?.content ?? [];
    const index = found.path.at(-1) ?? 0;
    const neighbour = siblings[index + 1] ?? siblings[index - 1];
    const doc = removeBlockAt(this.state.doc, found.path, this.ids);
    return this.commit({
      doc,
      structural: true,
      selectId: neighbour?.attrs?.blockId ? String(neighbour.attrs.blockId) : null,
      selection: null,
    });
  }

  moveBlock(delta: -1 | 1, blockId?: string): boolean {
    const id = this.target(blockId);
    const found = id ? findBlock(this.state.doc, id) : null;
    const doc = found ? moveBlockBy(this.state.doc, found.path, delta) : null;
    return doc ? this.commit({ doc, structural: true }) : false;
  }

  duplicateBlock(blockId?: string): string | null {
    const id = this.target(blockId);
    const found = id ? findBlock(this.state.doc, id) : null;
    if (!found) return null;
    const copy = cloneWithFreshIds(found.node, this.ids);
    const parentPath = found.path.slice(0, -1);
    const siblings = nodeAt(this.state.doc, parentPath)?.content ?? [];
    const doc = setChildren(
      this.state.doc,
      parentPath,
      insertAt(siblings, (found.path.at(-1) ?? 0) + 1, copy),
    );
    const newId = String(copy.attrs?.blockId);
    return this.commit({ doc, structural: true, selectId: newId, selection: null }) ? newId : null;
  }

  /** Splits a paragraph or heading at the caret; returns the block that receives the caret. */
  splitBlock(blockId: string, offset: number, to = offset): string | null {
    let doc = this.state.doc;
    const found = findBlock(doc, blockId);
    if (found && to > offset) {
      const trimmed = deleteRange(found.node.content ?? [], offset, to);
      doc = updateAtPath(doc, found.path, (node) => withInlines(node, "content", trimmed));
    }
    const edit = splitTextBlock(doc, blockId, offset, this.ids);
    if (!edit?.selectId) return null;
    const ok = this.commit({
      doc: edit.doc,
      structural: true,
      selectId: edit.selectId,
      selection: { blockId: edit.selectId, part: "content", from: edit.caret, to: edit.caret },
    });
    return ok ? edit.selectId : null;
  }

  /** Joins a text block onto the previous one (Backspace at the start). */
  mergeWithPrevious(blockId: string): boolean {
    const edit = mergeIntoPrevious(this.state.doc, blockId);
    if (!edit?.selectId) return false;
    return this.commit({
      doc: edit.doc,
      structural: true,
      selectId: edit.selectId,
      selection: { blockId: edit.selectId, part: "content", from: edit.caret, to: edit.caret },
    });
  }

  /** Joins the next text block onto this one (Delete at the end). */
  mergeWithNext(blockId: string): boolean {
    const next = siblingTextBlockId(this.state.doc, blockId, 1);
    return next ? this.mergeWithPrevious(next) : false;
  }

  // ---- tables -------------------------------------------------------------------------------------

  private editTable(tableId: string, change: (table: TiptapNode) => TiptapNode | null): boolean {
    const found = findBlock(this.state.doc, tableId);
    if (found?.node.type !== "table") return false;
    const next = change(found.node);
    if (!next) return false;
    return this.commit({ doc: updateAtPath(this.state.doc, found.path, () => next) });
  }

  addTableRow(tableId: string, section: "head" | "body" = "body", afterRow: number | null = null) {
    return this.editTable(tableId, (table) => addTableRow(table, section, afterRow, this.ids));
  }

  deleteTableRow(tableId: string, ref: Pick<TableCellRef, "section" | "row">) {
    return this.editTable(tableId, (table) => deleteTableRow(table, ref));
  }

  addTableColumn(tableId: string) {
    return this.editTable(tableId, (table) => addTableColumn(table, this.ids));
  }

  deleteTableColumn(tableId: string, ref: TableCellRef) {
    return this.editTable(tableId, (table) => deleteTableColumn(table, ref));
  }

  setTableCellSpan(
    tableId: string,
    ref: TableCellRef,
    span: { rowspan?: number; colspan?: number },
  ) {
    return this.editTable(tableId, (table) => setTableCellSpan(table, ref, span, this.ids));
  }

  toggleTableHeader(tableId: string) {
    return this.editTable(tableId, toggleHeaderRow);
  }

  // ---- styles -----------------------------------------------------------------------------------------

  /** Applies a named paragraph style, or clears it with `null`. */
  setBlockStyle(styleId: string | null, blockId?: string): boolean {
    const id = this.target(blockId);
    return id ? this.setBlockAttrs(id, { styleId }, `style:${id}`) : false;
  }

  /** Direct paragraph overrides; keys set to `undefined` are cleared. */
  setBlockDirect(patch: ParagraphStyle, blockId?: string): boolean {
    const id = this.target(blockId);
    const found = id ? findBlock(this.state.doc, id) : null;
    if (!id || !found) return false;
    const merged = patchStyleWithFont((found.node.attrs?.direct ?? {}) as ParagraphStyle, patch);
    return this.setBlockAttrs(
      id,
      { direct: merged },
      `direct:${id}:${Object.keys(patch).join(",")}`,
    );
  }

  clearBlockDirect(blockId?: string): boolean {
    const id = this.target(blockId);
    return id ? this.setBlockAttrs(id, { direct: {} }, `direct:${id}:clear`) : false;
  }

  private editStyles(
    change: (styles: NonNullable<IrDocument["styles"]>) => NonNullable<IrDocument["styles"]>,
    key: string,
    doc?: TiptapDoc,
  ): boolean {
    const styles = change({ ...this.state.original.styles });
    return this.commit({ original: { ...this.state.original, styles }, doc, key });
  }

  /** Creates or edits a named paragraph style in the document stylesheet. */
  updateParagraphStyle(styleId: string, patch: ParagraphStyle): boolean {
    if (!styleId.trim()) return false;
    return this.editStyles(
      (styles) => {
        const next = patchStyleWithFont(styles.paragraph?.[styleId], patch);
        return { ...styles, paragraph: { ...styles.paragraph, [styleId]: next } };
      },
      `paragraph-style:${styleId}:${Object.keys(patch).join(",")}`,
    );
  }

  updateCharacterStyle(styleId: string, patch: CharacterStyle): boolean {
    if (!styleId.trim()) return false;
    return this.editStyles(
      (styles) => {
        const next = patchStyleWithFont(styles.character?.[styleId], patch);
        return { ...styles, character: { ...styles.character, [styleId]: next } };
      },
      `character-style:${styleId}:${Object.keys(patch).join(",")}`,
    );
  }

  /** Deletes a paragraph style and clears every reference to it. */
  deleteParagraphStyle(styleId: string): boolean {
    if (!this.state.original.styles?.paragraph?.[styleId]) return false;
    let doc = this.state.doc;
    const blocks: string[] = [];
    walkBlocks(doc, (node) => {
      if (node.attrs?.styleId === styleId) blocks.push(String(node.attrs.blockId));
    });
    for (const id of blocks) {
      const found = findBlock(doc, id);
      if (found)
        doc = updateAtPath(doc, found.path, (node) => ({
          ...node,
          attrs: { ...node.attrs, styleId: null },
        }));
    }
    return this.editStyles(
      (styles) => {
        const paragraph = Object.fromEntries(
          Object.entries(styles.paragraph ?? {})
            .filter(([id]) => id !== styleId)
            .map(([id, style]) => [
              id,
              style.based_on === styleId
                ? (compactStyle({ ...style, based_on: undefined }) ?? {})
                : style,
            ]),
        );
        return { ...styles, paragraph };
      },
      `delete-style:${styleId}`,
      doc,
    );
  }

  // ---- export -----------------------------------------------------------------------------------------------

  /** Builds the IR for export. Throws when a section start was deleted or reordered. */
  exportIr(): IrDocument {
    // The mapper assigns missing block IDs in place, so it works on a copy of the tree.
    return editorStateToIr({ ...this.state, doc: structuredClone(this.state.doc) });
  }

  serializeIr(): string {
    return serializeEditorIr({ ...this.state, doc: structuredClone(this.state.doc) });
  }

  getChangeSummary(): EditorChangeSummary {
    return summarizeEditorChanges(this.baseline, this.exportIr());
  }

  buildExport(): ExportBundle {
    const ir = this.exportIr();
    return {
      ir,
      irJson: JSON.stringify(ir),
      summary: summarizeEditorChanges(this.baseline, ir),
    };
  }
}
