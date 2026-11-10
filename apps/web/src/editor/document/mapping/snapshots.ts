import type { EditorDocumentState, IrFields, TiptapNode } from "./types";

export function clone<T>(value: T): T {
  return structuredClone(value);
}

/** Object property order has no meaning in either wire contract. */
export function equal(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  if (!a || !b || typeof a !== "object" || typeof b !== "object") return false;
  if (Array.isArray(a) !== Array.isArray(b)) return false;
  const left = Object.keys(a);
  const right = Object.keys(b);
  return (
    left.length === right.length &&
    left.every((key) => Object.hasOwn(b, key) && equal((a as IrFields)[key], (b as IrFields)[key]))
  );
}

export function remember(
  state: EditorDocumentState,
  key: string,
  original: IrFields,
  node: TiptapNode,
): TiptapNode {
  node.attrs = { ...node.attrs, irKey: key };
  state.snapshots[key] = { original: clone(original), node: clone(node) };
  return node;
}

export function snapshot(state: EditorDocumentState, node: TiptapNode) {
  const key = node.attrs?.irKey;
  return typeof key === "string" && Object.hasOwn(state.snapshots, key)
    ? state.snapshots[key]
    : undefined;
}

/** Retain omitted versus explicit empty fields when an attribute has not changed. */
export function optionalField(result: IrFields, key: string, value: unknown, empty: unknown): void {
  if (equal(result[key] ?? empty, value ?? empty)) return;
  if (value === null || value === undefined) delete result[key];
  else result[key] = clone(value);
}
