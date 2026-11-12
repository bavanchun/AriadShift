import { clone, equal, optionalField, remember, snapshot } from "./snapshots";
import type {
  CharacterStyle,
  EditorDocumentState,
  IrInline,
  TiptapMark,
  TiptapNode,
} from "./types";

const inlineMarks: Record<string, string> = {
  emph: "italic",
  strong: "bold",
  strikeout: "strike",
  superscript: "superscript",
  subscript: "subscript",
  link: "link",
  styled: "irStyle",
  code: "code",
};

export function inlinesToNodes(
  values: IrInline[],
  state: EditorDocumentState,
  path: string,
): TiptapNode[] {
  return values.flatMap((inline, index) => {
    const key = `${path}/${index}`;
    const markType = inlineMarks[inline.type];
    if (markType) {
      const children =
        inline.type === "code"
          ? inline.text
            ? [{ type: "text", text: inline.text }]
            : []
          : inlinesToNodes(inline.content ?? [], state, `${key}/content`);
      if (!children.length) {
        return [remember(state, key, inline, { type: "irEmpty" })];
      }
      const mark: TiptapMark = { type: markType, attrs: { irKey: key } };
      if (inline.type === "link") {
        mark.attrs = { ...mark.attrs, href: inline.url, title: inline.title ?? null };
      } else if (inline.type === "styled") {
        mark.attrs = {
          ...mark.attrs,
          styleId: inline.style ?? null,
          direct: clone(inline.direct ?? {}),
        };
      }
      const nodes = children.map((node) => ({ ...node, marks: [mark, ...(node.marks ?? [])] }));
      state.snapshots[key] = {
        original: clone(inline),
        node: { type: "irSpan", content: clone(nodes) },
      };
      return nodes;
    }
    if (inline.type === "text" && inline.text) {
      const node: TiptapNode = { type: "text", text: inline.text };
      // Text nodes cannot have attributes in ProseMirror. Only extended text needs
      // a preservation mark; ordinary text remains native editor text.
      if (Object.keys(inline).some((field) => field !== "type" && field !== "text")) {
        node.marks = [{ type: "irPreserved", attrs: { irKey: key } }];
      }
      state.snapshots[key] = { original: clone(inline), node: clone(node) };
      return [node];
    }
    let node: TiptapNode;
    switch (inline.type) {
      case "text":
        node = { type: "irEmpty" };
        break;
      case "image":
        node = {
          type: "image",
          attrs: { target: clone(inline.target), alt: inline.alt, title: inline.title ?? null },
        };
        break;
      case "soft_break":
        node = { type: "softBreak" };
        break;
      case "line_break":
        node = { type: "hardBreak" };
        break;
      case "math":
        node = { type: "inlineMath", attrs: { tex: inline.tex, display: inline.display } };
        break;
      case "footnote_ref":
        node = { type: "footnoteRef", attrs: { noteId: inline.id } };
        break;
      case "raw":
        node = { type: "rawInline", attrs: { format: inline.format, text: inline.text } };
        break;
      default:
        node = { type: "irUnknownInline" };
    }
    return [remember(state, key, inline, node)];
  });
}

function markOriginal(state: EditorDocumentState, mark: TiptapMark): IrInline | undefined {
  const key = mark.attrs?.irKey;
  return typeof key === "string" && Object.hasOwn(state.snapshots, key)
    ? (clone(state.snapshots[key]?.original) as IrInline)
    : undefined;
}

function fromMark(mark: TiptapMark, nodes: TiptapNode[], state: EditorDocumentState): IrInline {
  const original = markOriginal(state, mark);
  const attrs = mark.attrs ?? {};
  const content = nodesToInlines(nodes, state);
  const type = Object.keys(inlineMarks).find((key) => inlineMarks[key] === mark.type);
  if (mark.type === "code" || mark.type === "irPreserved") {
    return { ...original, type: original?.type ?? "code", text: nodes.map(textOf).join("") };
  }
  if (type) {
    const result: IrInline = { ...original, type, content };
    if (type === "link") {
      result.url = String(attrs.href ?? "");
      optionalField(result, "title", attrs.title, null);
      if (!original) result.title = (attrs.title as string | null) ?? null;
    } else if (type === "styled") {
      optionalField(result, "style", attrs.styleId, null);
      optionalField(result, "direct", attrs.direct, {});
    }
    return result;
  }
  let direct: CharacterStyle;
  switch (mark.type) {
    case "underline":
      direct = { underline: true };
      break;
    case "highlight":
      direct = { highlight: String(attrs.color ?? "#ffff00") };
      break;
    case "textStyle":
      direct = clone((attrs.direct ?? {}) as CharacterStyle);
      if (typeof attrs.color === "string") direct.color = attrs.color;
      if (typeof attrs.fontFamily === "string") direct.font = { family: attrs.fontFamily };
      if (typeof attrs.sizePt === "number") direct.size_pt = attrs.sizePt;
      break;
    default:
      throw new Error(`Unsupported editor mark: ${mark.type}`);
  }
  return { type: "styled", direct, content };
}

export function textOf(node: TiptapNode): string {
  return node.text ?? (node.content ?? []).map(textOf).join("");
}

export function nodesToInlines(nodes: TiptapNode[], state: EditorDocumentState): IrInline[] {
  // ProseMirror sorts mark sets by schema rank. Original span keys retain the
  // source nesting order even after an editor JSON serialization reorders them.
  nodes = nodes.map((node) => {
    const marks = node.marks;
    if (
      !marks?.every(
        (mark) =>
          typeof mark.attrs?.irKey === "string" && Object.hasOwn(state.snapshots, mark.attrs.irKey),
      )
    )
      return node;
    return {
      ...node,
      marks: [...marks].sort(
        (a, b) => String(a.attrs?.irKey).length - String(b.attrs?.irKey).length,
      ),
    };
  });
  const result: IrInline[] = [];
  for (let index = 0; index < nodes.length; index++) {
    const node = nodes[index];
    if (!node) continue;
    const mark = node.marks?.[0];
    if (mark) {
      const group: TiptapNode[] = [];
      do {
        const current = nodes[index];
        if (!current) break;
        const { marks, ...rest } = current;
        group.push(marks && marks.length > 1 ? { ...rest, marks: marks.slice(1) } : rest);
        index++;
      } while (index < nodes.length && equal(nodes[index]?.marks?.[0], mark));
      index--;
      result.push(fromMark(mark, group, state));
      continue;
    }
    const saved = snapshot(state, node);
    if (saved && equal(saved.node, node)) {
      result.push(clone(saved.original) as IrInline);
      continue;
    }
    const inline = { ...clone(saved?.original) } as IrInline;
    const attrs = node.attrs ?? {};
    switch (node.type) {
      case "text":
        result.push({ type: "text", text: node.text ?? "" });
        continue;
      case "image":
        Object.assign(inline, {
          type: "image",
          target: attrs.target ?? { type: "url", href: attrs.src ?? "" },
          alt: attrs.alt ?? "",
          title: attrs.title ?? null,
        });
        break;
      case "softBreak":
        inline.type = "soft_break";
        break;
      case "hardBreak":
        inline.type = "line_break";
        break;
      case "inlineMath":
        Object.assign(inline, {
          type: "math",
          tex: attrs.tex ?? "",
          display: attrs.display ?? false,
        });
        break;
      case "footnoteRef":
        Object.assign(inline, { type: "footnote_ref", id: attrs.noteId ?? "" });
        break;
      case "rawInline":
        Object.assign(inline, {
          type: "raw",
          format: attrs.format ?? "html",
          text: attrs.text ?? "",
        });
        break;
      default:
        if (!saved) throw new Error(`Unsupported editor inline: ${node.type}`);
    }
    result.push(inline);
  }
  return result;
}
