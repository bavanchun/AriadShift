import { normalizeInlines } from "./inline-ops";
import type { TiptapMark, TiptapNode } from "./mapping";

/**
 * Bridge between the contentEditable DOM and the inline JSON model. Text runs are rendered inside
 * `data-marks` wrappers (the node's full mark list) and atoms carry their node as `data-atom`
 * with their length in `data-len`, so typing inherits formatting and nothing is lost on read-back.
 */

function parseJson<T>(value: string | null): T | null {
  if (value === null) return null;
  try {
    return JSON.parse(value) as T;
  } catch {
    return null;
  }
}

/** Reads the editable's DOM back into inline nodes. */
export function parseInlineDom(root: Element): TiptapNode[] {
  const out: TiptapNode[] = [];
  const walk = (node: Node, marks: TiptapMark[]) => {
    if (node.nodeType === 3) {
      const text = node.nodeValue ?? "";
      if (text) out.push(marks.length ? { type: "text", text, marks } : { type: "text", text });
      return;
    }
    if (node.nodeType !== 1) return;
    const element = node as Element;
    const own = parseJson<TiptapMark[]>(element.getAttribute("data-marks"));
    const current = own ?? marks;
    const atom = parseJson<TiptapNode>(element.getAttribute("data-atom"));
    if (atom) {
      out.push(current.length ? { ...atom, marks: current } : atom);
      return;
    }
    for (const child of Array.from(element.childNodes)) walk(child, current);
  };
  for (const child of Array.from(root.childNodes)) walk(child, []);
  return normalizeInlines(out);
}

function measure(node: Node): number {
  if (node.nodeType === 3) return (node.nodeValue ?? "").length;
  if (node.nodeType === 1) {
    const length = (node as Element).getAttribute("data-len");
    if (length !== null) return Number(length);
  }
  let total = 0;
  for (const child of Array.from(node.childNodes)) total += measure(child);
  return total;
}

/** Model offset of a DOM position inside `root`. */
export function domOffset(root: Element, node: Node, offset: number): number {
  const range = root.ownerDocument.createRange();
  range.selectNodeContents(root);
  try {
    range.setEnd(node, offset);
  } catch {
    return measure(root);
  }
  return measure(range.cloneContents());
}

/** DOM position of a model offset inside `root`. */
export function domPosition(root: Element, target: number): { node: Node; offset: number } {
  let remaining = target;
  let result: { node: Node; offset: number } | null = null;
  const walk = (node: Node): boolean => {
    if (node.nodeType === 3) {
      const length = (node.nodeValue ?? "").length;
      if (remaining <= length) {
        result = { node, offset: remaining };
        return true;
      }
      remaining -= length;
      return false;
    }
    if (node.nodeType === 1) {
      const raw = (node as Element).getAttribute("data-len");
      if (raw !== null) {
        const length = Number(raw);
        if (length === 0) return false;
        if (remaining < length && node.parentNode) {
          result = {
            node: node.parentNode,
            offset: Array.prototype.indexOf.call(node.parentNode.childNodes, node),
          };
          return true;
        }
        remaining -= length;
        return false;
      }
    }
    return Array.from(node.childNodes).some(walk);
  };
  walk(root);
  return result ?? { node: root, offset: root.childNodes.length };
}

export function readDomSelection(root: Element): { from: number; to: number } | null {
  const selection = root.ownerDocument.getSelection();
  if (!selection || selection.rangeCount === 0) return null;
  const range = selection.getRangeAt(0);
  if (!root.contains(range.startContainer) || !root.contains(range.endContainer)) return null;
  return {
    from: domOffset(root, range.startContainer, range.startOffset),
    to: domOffset(root, range.endContainer, range.endOffset),
  };
}

export function writeDomSelection(root: HTMLElement, from: number, to: number): void {
  const selection = root.ownerDocument.getSelection();
  if (!selection) return;
  const range = root.ownerDocument.createRange();
  const start = domPosition(root, from);
  const end = from === to ? start : domPosition(root, to);
  range.setStart(start.node, start.offset);
  range.setEnd(end.node, end.offset);
  selection.removeAllRanges();
  selection.addRange(range);
}
