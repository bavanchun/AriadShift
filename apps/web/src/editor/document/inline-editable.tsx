"use client";

import {
  type ClipboardEvent,
  type CSSProperties,
  type KeyboardEvent,
  type MouseEvent,
  memo,
  type ReactNode,
  useLayoutEffect,
  useRef,
} from "react";
import { fillLabel } from "./document-mode-labels";
import type { InlinePart } from "./document-tree";
import { type EditorContextValue, imageSource, safeHref, useEditorContext } from "./editor-context";
import { domOffset, parseInlineDom, readDomSelection, writeDomSelection } from "./inline-dom";
import { atomAt, inlineLength } from "./inline-ops";
import type { AssetRef, CharacterStyle, TiptapMark, TiptapNode } from "./mapping";
import { characterCss, resolveCharacterStyle } from "./style-utils";

function wrapMarks(marks: TiptapMark[], child: ReactNode, ctx: EditorContextValue): ReactNode {
  return [...marks].reverse().reduce<ReactNode>((inner, mark) => {
    const attrs = mark.attrs ?? {};
    switch (mark.type) {
      case "bold":
        return <strong>{inner}</strong>;
      case "italic":
        return <em>{inner}</em>;
      case "underline":
        return <u>{inner}</u>;
      case "strike":
        return <s>{inner}</s>;
      case "code":
        return <code className="rounded bg-muted px-1 font-mono text-[0.9em]">{inner}</code>;
      case "subscript":
        return <sub>{inner}</sub>;
      case "superscript":
        return <sup>{inner}</sup>;
      case "highlight":
        return <mark style={{ backgroundColor: String(attrs.color ?? "#ffff00") }}>{inner}</mark>;
      case "link":
        return (
          <a
            href={safeHref(attrs.href)}
            title={typeof attrs.title === "string" ? attrs.title : undefined}
            rel="noreferrer"
            className="text-primary underline underline-offset-2"
          >
            {inner}
          </a>
        );
      case "textStyle":
        return <span style={characterCss((attrs.direct ?? {}) as CharacterStyle)}>{inner}</span>;
      case "irStyle":
        return (
          <span
            style={characterCss(
              resolveCharacterStyle(
                ctx.styles,
                typeof attrs.styleId === "string" ? attrs.styleId : null,
                attrs.direct as CharacterStyle | undefined,
              ),
            )}
          >
            {inner}
          </span>
        );
      default:
        return inner;
    }
  }, child);
}

function atomView(node: TiptapNode, ctx: EditorContextValue): ReactNode {
  const attrs = node.attrs ?? {};
  const common = {
    "data-atom": JSON.stringify({ type: node.type, ...(node.attrs ? { attrs: node.attrs } : {}) }),
    "data-len": node.type === "irEmpty" ? "0" : "1",
    contentEditable: false as const,
  };
  switch (node.type) {
    case "hardBreak":
      return (
        <span {...common}>
          <br />
        </span>
      );
    case "softBreak":
      return (
        <span {...common} className="text-muted-foreground" title="Soft break">
          {" "}
        </span>
      );
    case "inlineMath":
      return (
        <span
          {...common}
          role="img"
          aria-label={`${ctx.labels.inlineMath}: ${String(attrs.tex ?? "")}`}
          className="cursor-pointer rounded bg-muted px-1 font-mono text-[0.9em]"
        >
          {String(attrs.tex ?? "") || "∅"}
        </span>
      );
    case "footnoteRef":
      return (
        <sup
          {...common}
          role="img"
          aria-label={fillLabel(ctx.labels.footnoteReference, { id: String(attrs.noteId ?? "") })}
          className="cursor-pointer text-primary"
        >
          [{String(attrs.noteId ?? "")}]
        </sup>
      );
    case "rawInline":
      return (
        <code
          {...common}
          className="cursor-pointer rounded border border-dashed border-border px-1 font-mono text-[0.9em]"
          title={String(attrs.format ?? "")}
        >
          {String(attrs.text ?? "")}
        </code>
      );
    case "image": {
      const src = imageSource(ctx.assets, attrs.target as AssetRef | undefined);
      return src ? (
        <img
          {...common}
          src={src}
          alt={String(attrs.alt ?? "")}
          className="inline-block max-h-40 align-middle"
        />
      ) : (
        <span {...common} className="text-muted-foreground">
          [{String(attrs.alt ?? "image")}]
        </span>
      );
    }
    case "irEmpty":
      return <span {...common} hidden />;
    default:
      return (
        <span {...common} className="text-muted-foreground" title="Preserved inline element">
          ⌧
        </span>
      );
  }
}

export function renderInlines(nodes: TiptapNode[], ctx: EditorContextValue): ReactNode[] {
  return nodes.map((node, index) => {
    const key = `${index}:${node.type}`;
    const marks = node.marks ?? [];
    const body: ReactNode =
      node.type === "text"
        ? wrapMarks(marks, node.text ?? "", ctx)
        : wrapMarks(marks, atomView(node, ctx), ctx);
    if (!marks.length)
      return (
        <span key={key} data-plain>
          {body}
        </span>
      );
    return (
      <span key={key} data-marks={JSON.stringify(marks)}>
        {body}
      </span>
    );
  });
}

interface InlineEditableProps {
  blockId: string;
  part: InlinePart;
  content: TiptapNode[];
  label: string;
  className?: string;
  style?: CSSProperties;
  /** Enter splits the block (paragraphs and headings); off for captions. */
  splittable?: boolean;
  placeholder?: string;
}

const isMod = (event: KeyboardEvent) => event.metaKey || event.ctrlKey;

/**
 * A contentEditable text part. While the user types, the browser owns the DOM and the parsed
 * nodes are written to the session; every other change re-renders it from the model.
 */
export const InlineEditable = memo(function InlineEditable({
  blockId,
  part,
  content,
  label,
  className,
  style,
  splittable = true,
  placeholder,
}: InlineEditableProps) {
  const ctx = useEditorContext();
  const ref = useRef<HTMLDivElement>(null);
  const emitted = useRef<TiptapNode[] | null>(null);
  const rendered = useRef<{ content: TiptapNode[]; key: number; elements: ReactNode[] } | null>(
    null,
  );

  if (!rendered.current) {
    rendered.current = { content, key: 1, elements: renderInlines(content, ctx) };
  } else if (content === emitted.current) {
    // The DOM already shows what the user typed; keep the browser's nodes.
    rendered.current.content = content;
  } else if (content !== rendered.current.content) {
    if (ref.current && ref.current.ownerDocument.activeElement === ref.current) {
      ctx.focusRequest.current ??= "selection";
    }
    emitted.current = null;
    rendered.current = {
      content,
      key: rendered.current.key + 1,
      elements: renderInlines(content, ctx),
    };
  }

  // Styles and assets change what a run looks like without changing the model content.
  const lastLook = useRef({ styles: ctx.styles, assets: ctx.assets });
  if (lastLook.current.styles !== ctx.styles || lastLook.current.assets !== ctx.assets) {
    lastLook.current = { styles: ctx.styles, assets: ctx.assets };
    if (ref.current && ref.current.ownerDocument.activeElement === ref.current) {
      ctx.focusRequest.current ??= "selection";
    }
    rendered.current = {
      content: rendered.current.content,
      key: rendered.current.key + 1,
      elements: renderInlines(rendered.current.content, ctx),
    };
  }

  const renderKey = rendered.current.key;
  useLayoutEffect(() => {
    const request = ctx.focusRequest.current;
    if (
      request !== null &&
      request !== "selection" &&
      request.blockId === blockId &&
      request.part === part
    ) {
      ref.current?.focus();
    }
  }, [renderKey]);

  const syncSelection = () => {
    const element = ref.current;
    const range = element ? readDomSelection(element) : null;
    if (range) ctx.session.setSelection({ blockId, part, ...range });
  };

  const nodesOf = () => {
    const element = ref.current;
    return element ? parseInlineDom(element) : [];
  };

  const goToNeighbour = (event: KeyboardEvent, delta: -1 | 1) => {
    const element = ref.current;
    const canvas = element?.closest("[data-document-canvas]");
    if (!element || !canvas) return;
    const editables = Array.from(canvas.querySelectorAll<HTMLElement>("[data-inline-editable]"));
    const next = editables[editables.indexOf(element) + delta];
    if (!next) return;
    event.preventDefault();
    next.focus();
    const length = parseInlineDom(next).reduce(
      (sum, node) => sum + (node.type === "text" ? (node.text?.length ?? 0) : 1),
      0,
    );
    writeDomSelection(next, delta === 1 ? 0 : length, delta === 1 ? 0 : length);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const element = ref.current;
    const range = element ? readDomSelection(element) : null;
    if (!element || !range || event.nativeEvent.isComposing || isMod(event) || event.altKey) {
      return;
    }
    const collapsed = range.from === range.to;
    const length = inlineLength(nodesOf());
    switch (event.key) {
      case "Enter":
        event.preventDefault();
        if (!splittable || part !== "content") return;
        ctx.focusRequest.current = "selection";
        if (event.shiftKey) {
          ctx.session.insertInline(blockId, part, range.from, { type: "hardBreak" }, range.to);
        } else if (!ctx.session.splitBlock(blockId, range.from, range.to)) {
          ctx.focusRequest.current = null;
        }
        return;
      case "Backspace":
        if (collapsed && range.from === 0 && part === "content") {
          ctx.focusRequest.current = "selection";
          if (ctx.session.mergeWithPrevious(blockId)) event.preventDefault();
          else ctx.focusRequest.current = null;
        }
        return;
      case "Delete":
        if (collapsed && range.from === length && part === "content") {
          ctx.focusRequest.current = "selection";
          if (ctx.session.mergeWithNext(blockId)) event.preventDefault();
          else ctx.focusRequest.current = null;
        }
        return;
      case "ArrowUp":
        if (collapsed && range.from === 0) goToNeighbour(event, -1);
        return;
      case "ArrowDown":
        if (collapsed && range.from === length) goToNeighbour(event, 1);
        return;
      case "F2": {
        const atom = atomAt(nodesOf(), range.from);
        if (atom && range.to - range.from <= 1) {
          event.preventDefault();
          ctx.activateAtom({ blockId, part, offset: range.from, node: atom });
        }
        return;
      }
      default:
    }
  };

  const onPaste = (event: ClipboardEvent<HTMLDivElement>) => {
    // Pasted markup never enters the document: only its text does.
    event.preventDefault();
    const element = ref.current;
    const range = element ? readDomSelection(element) : null;
    const text = event.clipboardData.getData("text/plain").replace(/\r?\n/g, " ");
    if (!range || !text) return;
    ctx.focusRequest.current = "selection";
    ctx.session.replaceText(blockId, part, range.from, range.to, text);
  };

  const onClick = (event: MouseEvent<HTMLDivElement>) => {
    const target = event.target as Element;
    if (target.closest("a")) event.preventDefault();
    const atomElement = target.closest("[data-atom]");
    const element = ref.current;
    if (!atomElement || !element) return;
    const offset = domOffset(element, atomElement, 0);
    const node = atomAt(nodesOf(), offset);
    if (node && ["inlineMath", "footnoteRef", "rawInline"].includes(node.type)) {
      ctx.activateAtom({ blockId, part, offset, node });
    }
  };

  return (
    // biome-ignore lint/a11y/useSemanticElements: a contentEditable region cannot be an input element
    <div
      key={renderKey}
      ref={ref}
      role="textbox"
      tabIndex={0}
      aria-multiline={splittable}
      aria-label={label}
      contentEditable
      suppressContentEditableWarning
      spellCheck
      data-inline-editable=""
      data-block-id={blockId}
      data-part={part}
      data-placeholder={placeholder}
      className={`min-h-[1.5em] whitespace-pre-wrap break-words outline-none empty:before:text-muted-foreground empty:before:content-[attr(data-placeholder)] focus-visible:ring-2 focus-visible:ring-ring/50 ${className ?? ""}`}
      style={style}
      onInput={() => {
        const nodes = nodesOf();
        emitted.current = nodes;
        ctx.session.setInlines(blockId, part, nodes);
        syncSelection();
      }}
      onKeyDown={onKeyDown}
      onKeyUp={syncSelection}
      onMouseUp={syncSelection}
      onFocus={syncSelection}
      onPaste={onPaste}
      onDrop={(event) => event.preventDefault()}
      onClick={onClick}
    >
      {rendered.current.elements}
    </div>
  );
});
