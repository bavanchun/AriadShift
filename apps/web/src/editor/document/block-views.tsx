"use client";

import { Button } from "@ariadshift/ui/components/button";
import { Columns3Icon, Rows3Icon, Table2Icon, Trash2Icon } from "lucide-react";
import { memo, type ReactNode, useState, useSyncExternalStore } from "react";
import { fillLabel } from "./document-mode-labels";
import { rowsOfSection, type TableCellRef } from "./document-tree";
import { imageSource, useEditorContext } from "./editor-context";
import { InlineEditable } from "./inline-editable";
import { plainText } from "./inline-ops";
import type { AssetRef, ParagraphStyle, TiptapNode } from "./mapping";
import { paragraphCss, resolveParagraphStyle } from "./style-utils";

const headingClasses = [
  "",
  "text-3xl font-bold",
  "text-2xl font-bold",
  "text-xl font-semibold",
  "text-lg font-semibold",
  "text-base font-semibold",
  "text-sm font-semibold",
];

function useBlockSelection(blockId: string) {
  const { session } = useEditorContext();
  const selected = useSyncExternalStore(
    session.subscribe,
    () => session.getSnapshot().selectedBlockId === blockId,
    () => false,
  );
  const inChain = useSyncExternalStore(
    session.subscribe,
    () => session.getSnapshot().selectedChain.includes(blockId),
    () => false,
  );
  return { selected, inChain };
}

function BlockFrame({
  node,
  className = "",
  children,
}: {
  node: TiptapNode;
  className?: string;
  children: ReactNode;
}) {
  const { session } = useEditorContext();
  const blockId = String(node.attrs?.blockId);
  const { selected } = useBlockSelection(blockId);
  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: selection follows focus and pointer events from nested editables
    <div
      role="presentation"
      data-block-id={blockId}
      data-block-type={node.type}
      data-selected={selected ? "" : undefined}
      className={`rounded-md px-2 py-1 transition-shadow ${selected ? "ring-2 ring-primary/40" : ""} ${className}`}
      onFocus={(event) => {
        event.stopPropagation();
        session.selectBlock(blockId);
      }}
      onMouseDown={(event) => {
        event.stopPropagation();
        session.selectBlock(blockId);
      }}
    >
      {children}
    </div>
  );
}

export function BlockList({ nodes }: { nodes: TiptapNode[] }) {
  return (
    <>
      {nodes.map((node) =>
        node.attrs?.blockId ? <BlockView key={String(node.attrs.blockId)} node={node} /> : null,
      )}
    </>
  );
}

function useParagraphCss(node: TiptapNode) {
  const { styles } = useEditorContext();
  return paragraphCss(
    resolveParagraphStyle(
      styles,
      typeof node.attrs?.styleId === "string" ? node.attrs.styleId : null,
      node.attrs?.direct as ParagraphStyle | undefined,
    ),
  );
}

function TextBlock({ node }: { node: TiptapNode }) {
  const { labels } = useEditorContext();
  const style = useParagraphCss(node);
  const blockId = String(node.attrs?.blockId);
  const level = Math.min(6, Math.max(1, Number(node.attrs?.level ?? 1)));
  const isHeading = node.type === "heading";
  const label = isHeading ? fillLabel(labels.heading, { level }) : labels.paragraph;
  const editable = (
    <InlineEditable
      blockId={blockId}
      part="content"
      content={node.content ?? []}
      label={label}
      placeholder={labels.emptyBlock}
      className={isHeading ? headingClasses[level] : ""}
      style={style}
    />
  );
  return (
    <BlockFrame node={node}>
      {isHeading ? (
        // biome-ignore lint/a11y/useSemanticElements: the editable inside must stay a textbox
        <div role="heading" aria-level={level}>
          {editable}
        </div>
      ) : (
        editable
      )}
    </BlockFrame>
  );
}

function ListBlock({ node }: { node: TiptapNode }) {
  const { session, labels } = useEditorContext();
  const blockId = String(node.attrs?.blockId);
  const ordered = node.type === "orderedList";
  const items = (node.content ?? []).map((item, index) => {
    const checked = item.attrs?.checked;
    return (
      <li key={index} className="flex items-start gap-2">
        {typeof checked === "boolean" && (
          <input
            type="checkbox"
            aria-label={labels.taskItem}
            checked={checked}
            className="mt-1.5"
            onChange={(event) => session.setListItemChecked(blockId, index, event.target.checked)}
          />
        )}
        <div className="min-w-0 flex-1">
          <BlockList nodes={item.content ?? []} />
        </div>
      </li>
    );
  });
  const start = typeof node.attrs?.start === "number" ? node.attrs.start : undefined;
  return (
    <BlockFrame node={node}>
      {ordered ? (
        <ol start={start} className="list-decimal ps-6">
          {items}
        </ol>
      ) : (
        <ul className="list-disc ps-6">{items}</ul>
      )}
    </BlockFrame>
  );
}

function TableBlock({ node }: { node: TiptapNode }) {
  const ctx = useEditorContext();
  const { session, labels } = ctx;
  const blockId = String(node.attrs?.blockId);
  const { inChain } = useBlockSelection(blockId);
  const children = node.content ?? [];
  const caption = children.find((child) => child.type === "tableCaption");
  const footnotes = children.find((child) => child.type === "tableFootnotes");
  const head = rowsOfSection(node, "head");
  const body = rowsOfSection(node, "body");
  const [active, setActive] = useState<TableCellRef>({
    section: head.length ? "head" : "body",
    row: 0,
    column: 0,
  });
  const columns = Array.isArray(node.attrs?.columns)
    ? (node.attrs.columns as { align?: string }[])
    : [];
  const activeCell = rowsOfSection(node, active.section)[active.row]?.content?.[active.column];

  const run = (ok: boolean) => {
    if (!ok) ctx.announce(labels.tableOperationUnavailable);
  };

  const renderRows = (rows: TiptapNode[], section: "head" | "body") =>
    rows.map((row, rowIndex) => (
      <tr key={rowIndex}>
        {(row.content ?? []).map((cell, column) => {
          const Tag = cell.type === "tableHeader" ? "th" : "td";
          const align = columns[column]?.align;
          const isActive =
            active.section === section && active.row === rowIndex && active.column === column;
          return (
            <Tag
              key={column}
              rowSpan={Number(cell.attrs?.rowspan ?? 1)}
              colSpan={Number(cell.attrs?.colspan ?? 1)}
              scope={Tag === "th" ? "col" : undefined}
              data-active-cell={isActive ? "" : undefined}
              className={`border border-border p-1 align-top ${Tag === "th" ? "bg-muted font-semibold" : ""}`}
              style={{ textAlign: align && align !== "default" ? (align as "left") : undefined }}
              onFocus={() => setActive({ section, row: rowIndex, column })}
              onClick={() => setActive({ section, row: rowIndex, column })}
            >
              <BlockList nodes={cell.content ?? []} />
            </Tag>
          );
        })}
      </tr>
    ));

  return (
    <BlockFrame node={node}>
      <table className="w-full table-fixed border-collapse text-sm">
        {caption && (
          <caption className="mb-1 text-start text-sm text-muted-foreground">
            <InlineEditable
              blockId={blockId}
              part="caption"
              content={caption.content ?? []}
              label={labels.tableCaption}
              splittable={false}
            />
          </caption>
        )}
        {head.length > 0 && <thead>{renderRows(head, "head")}</thead>}
        <tbody>{renderRows(body, "body")}</tbody>
      </table>
      {footnotes && (
        <div className="mt-1 text-xs text-muted-foreground">
          <InlineEditable
            blockId={blockId}
            part="footnotes"
            content={footnotes.content ?? []}
            label={fillLabel(labels.footnote, { id: "" }).trim()}
            splittable={false}
          />
        </div>
      )}
      {inChain && (
        <fieldset
          aria-label={labels.tableTools}
          className="m-0 mt-2 flex min-w-0 flex-wrap items-center gap-1 border-0 border-t border-border p-0 pt-2"
        >
          <Table2Icon aria-hidden="true" className="size-4 text-muted-foreground" />
          <Button
            type="button"
            variant="outline"
            size="xs"
            onClick={() => run(session.addTableRow(blockId, active.section, active.row))}
          >
            <Rows3Icon aria-hidden="true" />
            {labels.addRow}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="xs"
            onClick={() => run(session.addTableColumn(blockId))}
          >
            <Columns3Icon aria-hidden="true" />
            {labels.addColumn}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="xs"
            onClick={() => run(session.deleteTableRow(blockId, active))}
          >
            <Trash2Icon aria-hidden="true" />
            {labels.deleteRow}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="xs"
            onClick={() => run(session.deleteTableColumn(blockId, active))}
          >
            <Trash2Icon aria-hidden="true" />
            {labels.deleteColumn}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="xs"
            onClick={() => run(session.toggleTableHeader(blockId))}
          >
            {labels.toggleHeaderRow}
          </Button>
          {(["rowspan", "colspan"] as const).map((field) => (
            <label key={field} className="flex items-center gap-1 text-xs">
              {field === "rowspan" ? labels.rowSpan : labels.columnSpan}
              <input
                type="number"
                min={1}
                max={100}
                className="h-6 w-14 rounded border border-border bg-background px-1"
                value={Number(activeCell?.attrs?.[field] ?? 1)}
                onChange={(event) => {
                  const value = Number(event.target.value);
                  if (Number.isFinite(value)) {
                    run(session.setTableCellSpan(blockId, active, { [field]: value }));
                  }
                }}
              />
            </label>
          ))}
        </fieldset>
      )}
    </BlockFrame>
  );
}

function FigureBlock({ node }: { node: TiptapNode }) {
  const { assets, labels } = useEditorContext();
  const blockId = String(node.attrs?.blockId);
  const caption = (node.content ?? []).find((child) => child.type === "figureCaption");
  const src = imageSource(assets, node.attrs?.asset as AssetRef | undefined);
  return (
    <BlockFrame node={node}>
      <figure className="flex flex-col items-center gap-1">
        {src ? (
          <img src={src} alt={plainText(caption?.content ?? [])} className="max-h-96 max-w-full" />
        ) : (
          <div className="flex h-24 w-full items-center justify-center rounded border border-dashed border-border text-sm text-muted-foreground">
            {labels.imageUnavailable}
          </div>
        )}
        <figcaption className="w-full text-center text-sm text-muted-foreground">
          <InlineEditable
            blockId={blockId}
            part="caption"
            content={caption?.content ?? []}
            label={labels.figureCaption}
            splittable={false}
            placeholder={labels.figureCaption}
          />
        </figcaption>
      </figure>
    </BlockFrame>
  );
}

function MathBlock({ node }: { node: TiptapNode }) {
  const { session, labels } = useEditorContext();
  const blockId = String(node.attrs?.blockId);
  const tex = String(node.attrs?.tex ?? "");
  const display = node.attrs?.display !== false;
  return (
    <BlockFrame node={node}>
      <div className="flex flex-col gap-1">
        <div
          role="math"
          aria-label={`${labels.mathBlock}: ${tex}`}
          className={`rounded bg-muted/50 px-3 py-2 font-mono ${display ? "text-center" : ""}`}
        >
          {display ? `$$ ${tex} $$` : `$ ${tex} $`}
        </div>
        <textarea
          aria-label={labels.mathSource}
          value={tex}
          rows={Math.max(2, tex.split("\n").length)}
          spellCheck={false}
          className="w-full rounded border border-border bg-background p-2 font-mono text-sm"
          onChange={(event) => session.setMathSource(blockId, event.target.value)}
        />
        <label className="flex items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={display}
            onChange={(event) => session.setMathSource(blockId, tex, event.target.checked)}
          />
          {labels.mathDisplay}
        </label>
      </div>
    </BlockFrame>
  );
}

function CodeBlock({ node }: { node: TiptapNode }) {
  const { session, labels } = useEditorContext();
  const blockId = String(node.attrs?.blockId);
  const text = plainText(node.content ?? []);
  return (
    <BlockFrame node={node}>
      <div className="flex flex-col gap-1">
        <input
          aria-label={labels.codeLanguage}
          value={String(node.attrs?.language ?? "")}
          placeholder={labels.codeLanguage}
          className="h-6 w-40 rounded border border-border bg-background px-1 text-xs"
          onChange={(event) =>
            session.setBlockAttrs(blockId, { language: event.target.value || null })
          }
        />
        <textarea
          aria-label={labels.codeBlock}
          value={text}
          rows={Math.max(2, text.split("\n").length)}
          spellCheck={false}
          className="w-full rounded border border-border bg-muted/40 p-2 font-mono text-sm"
          onChange={(event) => session.setBlockText(blockId, event.target.value)}
        />
      </div>
    </BlockFrame>
  );
}

function RawBlock({ node }: { node: TiptapNode }) {
  const { session, labels } = useEditorContext();
  const blockId = String(node.attrs?.blockId);
  const text = plainText(node.content ?? []);
  return (
    <BlockFrame node={node}>
      <div className="flex flex-col gap-1 rounded border border-dashed border-border p-2">
        <p className="text-xs font-medium text-muted-foreground">
          {fillLabel(labels.rawBlock, { format: String(node.attrs?.format ?? "") })}
        </p>
        <p className="text-xs text-muted-foreground">{labels.rawBlockNotice}</p>
        <textarea
          aria-label={fillLabel(labels.rawBlock, { format: String(node.attrs?.format ?? "") })}
          value={text}
          rows={Math.max(2, text.split("\n").length)}
          spellCheck={false}
          className="w-full rounded border border-border bg-background p-2 font-mono text-sm"
          onChange={(event) => session.setBlockText(blockId, event.target.value)}
        />
      </div>
    </BlockFrame>
  );
}

function FootnoteBlock({ node }: { node: TiptapNode }) {
  const { labels } = useEditorContext();
  const noteId = String(node.attrs?.noteId ?? "");
  const label = fillLabel(labels.footnote, { id: noteId });
  return (
    <BlockFrame node={node}>
      <aside
        aria-label={label}
        className="flex gap-2 border-s-2 border-border ps-2 text-sm text-muted-foreground"
      >
        <span className="font-medium">[{noteId}]</span>
        <div className="min-w-0 flex-1">
          <BlockList nodes={node.content ?? []} />
        </div>
      </aside>
    </BlockFrame>
  );
}

function BlockSwitch({ node }: { node: TiptapNode }) {
  const { labels } = useEditorContext();
  switch (node.type) {
    case "paragraph":
    case "heading":
      return <TextBlock node={node} />;
    case "bulletList":
    case "orderedList":
      return <ListBlock node={node} />;
    case "blockquote":
      return (
        <BlockFrame node={node}>
          <blockquote className="border-s-4 border-border ps-3 italic">
            <BlockList nodes={node.content ?? []} />
          </blockquote>
        </BlockFrame>
      );
    case "codeBlock":
      return <CodeBlock node={node} />;
    case "blockMath":
      return <MathBlock node={node} />;
    case "table":
      return <TableBlock node={node} />;
    case "figure":
      return <FigureBlock node={node} />;
    case "footnote":
      return <FootnoteBlock node={node} />;
    case "rawBlock":
      return <RawBlock node={node} />;
    case "pageBreak":
      return (
        <BlockFrame node={node}>
          <div className="my-2 border-t border-dashed border-border text-center text-xs text-muted-foreground">
            {labels.pageBreak}
          </div>
        </BlockFrame>
      );
    default:
      return (
        <BlockFrame node={node}>
          <p className="rounded border border-dashed border-border p-2 text-sm text-muted-foreground">
            {labels.unknownBlock}
          </p>
        </BlockFrame>
      );
  }
}

export const BlockView = memo(BlockSwitch);
