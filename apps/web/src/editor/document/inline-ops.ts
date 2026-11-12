import type { CharacterStyle, StyleSheet, TiptapMark, TiptapNode } from "./mapping";
import { compactStyle, patchStyle, resolveCharacterStyle } from "./style-utils";

/**
 * Pure, offset-based operations over a flat inline node list. Offsets count UTF-16 code units
 * of text plus one unit per atom (images, breaks, math, footnote references, raw inlines).
 * Marks are kept in the order the IR mapper expects: original spans (with `irKey`) outermost,
 * editor marks in a fixed rank next, and leaf marks that swallow their children innermost.
 */

export type MarkKind =
  | "bold"
  | "italic"
  | "underline"
  | "strike"
  | "code"
  | "subscript"
  | "superscript"
  | "link"
  | "highlight";

export const MARK_KINDS: readonly MarkKind[] = [
  "bold",
  "italic",
  "underline",
  "strike",
  "code",
  "subscript",
  "superscript",
  "link",
  "highlight",
];

const RANK: Record<string, number> = {
  link: 0,
  bold: 1,
  italic: 2,
  underline: 3,
  strike: 4,
  subscript: 5,
  superscript: 5,
  textStyle: 6,
  irStyle: 7,
  highlight: 8,
};

const DIRECT_EQUIVALENT: Partial<Record<MarkKind, (style: CharacterStyle) => boolean>> = {
  bold: (style) => (style.weight ?? 400) >= 600,
  italic: (style) => style.italic === true,
  underline: (style) => style.underline === true,
  strike: (style) => style.strike === true,
};

const DIRECT_KEY: Partial<Record<MarkKind, keyof CharacterStyle>> = {
  bold: "weight",
  italic: "italic",
  underline: "underline",
  strike: "strike",
};

export function deepEqual(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (!a || !b || typeof a !== "object" || typeof b !== "object") return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const left = Object.keys(a);
  const right = Object.keys(b);
  return (
    left.length === right.length &&
    left.every(
      (key) =>
        Object.hasOwn(b, key) &&
        deepEqual((a as Record<string, unknown>)[key], (b as Record<string, unknown>)[key]),
    )
  );
}

const hasKey = (mark: TiptapMark) => typeof mark.attrs?.irKey === "string";
const isLeafMark = (mark: TiptapMark) => mark.type === "irPreserved" || mark.type === "code";

export function nodeLength(node: TiptapNode): number {
  if (node.type === "text") return node.text?.length ?? 0;
  return node.type === "irEmpty" ? 0 : 1;
}

export function inlineLength(nodes: TiptapNode[]): number {
  return nodes.reduce((total, node) => total + nodeLength(node), 0);
}

/** Text with breaks as characters; other atoms contribute nothing. */
export function plainText(nodes: TiptapNode[]): string {
  return nodes
    .map((node) => {
      if (node.type === "text") return node.text ?? "";
      if (node.type === "hardBreak") return "\n";
      if (node.type === "softBreak") return " ";
      return "";
    })
    .join("");
}

export function normalizeMarks(marks: TiptapMark[] | undefined): TiptapMark[] | undefined {
  if (!marks?.length) return undefined;
  const keyed = marks.filter((mark) => hasKey(mark) && !isLeafMark(mark));
  const editor = marks
    .filter((mark) => !hasKey(mark) && !isLeafMark(mark))
    .sort((a, b) => (RANK[a.type] ?? 9) - (RANK[b.type] ?? 9));
  const unique = editor.filter(
    (mark, index) => index === 0 || mark.type !== editor[index - 1]?.type,
  );
  return [...keyed, ...unique, ...marks.filter(isLeafMark)];
}

function withMarks(node: TiptapNode, marks: TiptapMark[] | undefined): TiptapNode {
  const { marks: _previous, ...rest } = node;
  const next = normalizeMarks(marks);
  return next ? { ...rest, marks: next } : rest;
}

/** Merges neighbouring text runs that carry identical marks and drops empty text. */
export function normalizeInlines(nodes: TiptapNode[]): TiptapNode[] {
  const result: TiptapNode[] = [];
  for (const node of nodes) {
    if (node.type === "text" && !node.text) continue;
    const last = result[result.length - 1];
    if (
      node.type === "text" &&
      last?.type === "text" &&
      deepEqual(last.marks ?? [], node.marks ?? [])
    ) {
      result[result.length - 1] = { ...last, text: (last.text ?? "") + (node.text ?? "") };
    } else {
      result.push(node);
    }
  }
  return result;
}

export function splitInlines(
  nodes: TiptapNode[],
  offset: number,
): [before: TiptapNode[], after: TiptapNode[]] {
  const before: TiptapNode[] = [];
  const after: TiptapNode[] = [];
  let position = 0;
  for (const node of nodes) {
    const length = nodeLength(node);
    if (length === 0) {
      (position <= offset ? before : after).push(node);
      continue;
    }
    const end = position + length;
    if (end <= offset) before.push(node);
    else if (position >= offset) after.push(node);
    else {
      const cut = offset - position;
      before.push({ ...node, text: (node.text ?? "").slice(0, cut) });
      after.push({ ...node, text: (node.text ?? "").slice(cut) });
    }
    position = end;
  }
  return [before, after];
}

export function sliceInlines(nodes: TiptapNode[], from: number, to: number): TiptapNode[] {
  const [, rest] = splitInlines(nodes, from);
  return splitInlines(rest, to - from)[0];
}

function clampRange(nodes: TiptapNode[], from: number, to: number): [number, number] {
  const length = inlineLength(nodes);
  const start = Math.max(0, Math.min(from, to, length));
  return [start, Math.max(start, Math.min(Math.max(from, to), length))];
}

function mapRange(
  nodes: TiptapNode[],
  from: number,
  to: number,
  change: (node: TiptapNode) => TiptapNode,
): TiptapNode[] {
  const [start, end] = clampRange(nodes, from, to);
  const [head, rest] = splitInlines(nodes, start);
  const [middle, tail] = splitInlines(rest, end - start);
  return normalizeInlines([
    ...head,
    ...middle.map((node) => (nodeLength(node) > 0 ? change(node) : node)),
    ...tail,
  ]);
}

export function deleteRange(nodes: TiptapNode[], from: number, to: number): TiptapNode[] {
  const [start, end] = clampRange(nodes, from, to);
  const [head, rest] = splitInlines(nodes, start);
  return normalizeInlines([...head, ...splitInlines(rest, end - start)[1]]);
}

function marksAround(nodes: TiptapNode[], offset: number, preferNext: boolean): TiptapMark[] {
  let position = 0;
  let before: TiptapNode | undefined;
  let after: TiptapNode | undefined;
  for (const node of nodes) {
    const length = nodeLength(node);
    if (length === 0) continue;
    if (position + length <= offset) before = node;
    else if (position >= offset) {
      after ??= node;
      break;
    } else {
      before = node;
      after = node;
      break;
    }
    position += length;
  }
  const source = preferNext ? (after ?? before) : (before ?? after);
  return source?.marks ?? [];
}

export function insertText(nodes: TiptapNode[], offset: number, text: string): TiptapNode[] {
  if (!text) return nodes;
  const [position] = clampRange(nodes, offset, offset);
  const marks = marksAround(nodes, position, false);
  const [head, tail] = splitInlines(nodes, position);
  return normalizeInlines([...head, withMarks({ type: "text", text }, marks), ...tail]);
}

export function replaceRange(
  nodes: TiptapNode[],
  from: number,
  to: number,
  text: string,
): TiptapNode[] {
  const [start, end] = clampRange(nodes, from, to);
  if (end === start) return insertText(nodes, start, text);
  const marks = marksAround(nodes, start, true);
  const [head, rest] = splitInlines(nodes, start);
  const tail = splitInlines(rest, end - start)[1];
  const inserted = text ? [withMarks({ type: "text", text }, marks)] : [];
  return normalizeInlines([...head, ...inserted, ...tail]);
}

export function insertInlineNode(
  nodes: TiptapNode[],
  offset: number,
  node: TiptapNode,
): TiptapNode[] {
  const [position] = clampRange(nodes, offset, offset);
  const [head, tail] = splitInlines(nodes, position);
  return normalizeInlines([...head, node, ...tail]);
}

/** Position of an atom and its node, for editing math, footnote references and raw inlines. */
export function atomAt(nodes: TiptapNode[], offset: number): TiptapNode | undefined {
  let position = 0;
  for (const node of nodes) {
    const length = nodeLength(node);
    if (node.type !== "text" && length === 1 && position === offset) return node;
    position += length;
  }
  return undefined;
}

export function updateAtom(
  nodes: TiptapNode[],
  offset: number,
  attrs: Record<string, unknown>,
): TiptapNode[] {
  let position = 0;
  return nodes.map((node) => {
    const length = nodeLength(node);
    const hit = node.type !== "text" && length === 1 && position === offset;
    position += length;
    return hit ? { ...node, attrs: { ...node.attrs, ...attrs } } : node;
  });
}

function nodesInRange(nodes: TiptapNode[], from: number, to: number): TiptapNode[] {
  const [start, end] = clampRange(nodes, from, to);
  if (start === end) {
    const around = marksAround(nodes, start, false);
    return [{ type: "text", text: "x", marks: around }];
  }
  return sliceInlines(nodes, start, end).filter((node) => nodeLength(node) > 0);
}

function markEquivalent(mark: TiptapMark, kind: MarkKind, styles: StyleSheet | undefined): boolean {
  if (mark.type === kind) return true;
  const test = DIRECT_EQUIVALENT[kind];
  if (!test || (mark.type !== "irStyle" && mark.type !== "textStyle")) return false;
  const styleId = typeof mark.attrs?.styleId === "string" ? mark.attrs.styleId : null;
  return test(resolveCharacterStyle(styles, styleId, mark.attrs?.direct as CharacterStyle));
}

export function markActive(
  nodes: TiptapNode[],
  from: number,
  to: number,
  kind: MarkKind,
  styles?: StyleSheet,
): boolean {
  const range = nodesInRange(nodes, from, to);
  return (
    range.length > 0 &&
    range.every((node) => (node.marks ?? []).some((mark) => markEquivalent(mark, kind, styles)))
  );
}

export function activeMarks(
  nodes: TiptapNode[],
  from: number,
  to: number,
  styles?: StyleSheet,
): Set<MarkKind> {
  return new Set(MARK_KINDS.filter((kind) => markActive(nodes, from, to, kind, styles)));
}

function stripKind(marks: TiptapMark[], kind: MarkKind): TiptapMark[] {
  const key = DIRECT_KEY[kind];
  return marks.flatMap((mark) => {
    if (mark.type === kind) return [];
    if (key && (mark.type === "irStyle" || mark.type === "textStyle")) {
      const direct = { ...(mark.attrs?.direct as CharacterStyle | undefined) };
      if (key in direct) {
        delete direct[key];
        return [{ ...mark, attrs: { ...mark.attrs, direct } }];
      }
    }
    return [mark];
  });
}

export interface ToggleMarkOptions {
  /** Link target or highlight colour. */
  attrs?: Record<string, unknown>;
  styles?: StyleSheet;
  /** Forces add or remove instead of toggling on the current state. */
  force?: "add" | "remove";
}

export function toggleMark(
  nodes: TiptapNode[],
  from: number,
  to: number,
  kind: MarkKind,
  options: ToggleMarkOptions = {},
): TiptapNode[] {
  const remove =
    options.force === "remove" ||
    (options.force !== "add" && markActive(nodes, from, to, kind, options.styles));
  return mapRange(nodes, from, to, (node) => {
    let marks = node.marks ?? [];
    if (remove) return withMarks(node, stripKind(marks, kind));
    if (kind === "code" && node.type !== "text") return node;
    if (kind === "subscript") marks = stripKind(marks, "superscript");
    if (kind === "superscript") marks = stripKind(marks, "subscript");
    // Only editor marks of the same kind are replaced; direct formatting stays as it is.
    marks = marks.filter((mark) => mark.type !== kind);
    const mark: TiptapMark = options.attrs ? { type: kind, attrs: options.attrs } : { type: kind };
    return withMarks(node, [...marks, mark]);
  });
}

export function linkAt(
  nodes: TiptapNode[],
  from: number,
  to: number,
): { href: string; title: string | null } | null {
  const mark = nodesInRange(nodes, from, to)
    .flatMap((node) => node.marks ?? [])
    .find((candidate) => candidate.type === "link");
  return mark
    ? { href: String(mark.attrs?.href ?? ""), title: (mark.attrs?.title as string | null) ?? null }
    : null;
}

export function setLink(
  nodes: TiptapNode[],
  from: number,
  to: number,
  link: { href: string; title?: string | null },
): TiptapNode[] {
  return toggleMark(nodes, from, to, "link", {
    force: "add",
    attrs: { href: link.href, title: link.title ?? null },
  });
}

export function removeLink(nodes: TiptapNode[], from: number, to: number): TiptapNode[] {
  return toggleMark(nodes, from, to, "link", { force: "remove" });
}

function isEditorMark(mark: TiptapMark, type: string) {
  return mark.type === type && !hasKey(mark);
}

/** Direct character formatting; keys set to `undefined` are removed. */
export function applyTextStyle(
  nodes: TiptapNode[],
  from: number,
  to: number,
  patch: CharacterStyle,
): TiptapNode[] {
  return mapRange(nodes, from, to, (node) => {
    const marks = node.marks ?? [];
    const current = marks.find((mark) => isEditorMark(mark, "textStyle"));
    const direct = compactStyle(
      patchStyle((current?.attrs?.direct as CharacterStyle | undefined) ?? {}, patch),
    );
    const others = marks.filter((mark) => mark !== current);
    return withMarks(node, direct ? [...others, { type: "textStyle", attrs: { direct } }] : others);
  });
}

/** Applies or clears a named character style. */
export function applyCharacterStyleId(
  nodes: TiptapNode[],
  from: number,
  to: number,
  styleId: string | null,
): TiptapNode[] {
  return mapRange(nodes, from, to, (node) => {
    const marks = (node.marks ?? [])
      .filter((mark) => !isEditorMark(mark, "irStyle"))
      .map((mark) =>
        styleId === null && mark.type === "irStyle"
          ? { ...mark, attrs: { ...mark.attrs, styleId: null } }
          : mark,
      );
    return withMarks(
      node,
      styleId === null ? marks : [...marks, { type: "irStyle", attrs: { styleId, direct: {} } }],
    );
  });
}

/** The resolved character style at a range start, used to show font controls. */
export function characterStyleAt(
  nodes: TiptapNode[],
  from: number,
  to: number,
  styles?: StyleSheet,
): CharacterStyle {
  const node = nodesInRange(nodes, from, to)[0];
  let result: CharacterStyle = {};
  for (const mark of node?.marks ?? []) {
    if (mark.type !== "irStyle" && mark.type !== "textStyle") continue;
    const styleId = typeof mark.attrs?.styleId === "string" ? mark.attrs.styleId : null;
    result = {
      ...result,
      ...resolveCharacterStyle(styles, styleId, mark.attrs?.direct as CharacterStyle),
    };
  }
  return result;
}

export function characterStyleIdAt(nodes: TiptapNode[], from: number, to: number): string | null {
  const node = nodesInRange(nodes, from, to)[0];
  const mark = (node?.marks ?? []).find(
    (candidate) => candidate.type === "irStyle" && typeof candidate.attrs?.styleId === "string",
  );
  return (mark?.attrs?.styleId as string | undefined) ?? null;
}
