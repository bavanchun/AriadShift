"use client";

import { Button } from "@ariadshift/ui/components/button";
import { XIcon } from "lucide-react";
import { type DocumentModeLabels, fillLabel } from "./document-mode-labels";
import type { DocumentEditSession, SessionSnapshot } from "./document-session";
import { findBlock, getInlines } from "./document-tree";
import type { ActiveAtom } from "./editor-context";
import { atomAt } from "./inline-ops";

const inputClass = "h-7 rounded-md border border-border bg-background px-2 text-sm";

/** Edits one inline atom: inline math TeX, a raw inline's text, or shows a footnote reference. */
export function AtomEditor({
  atom,
  snapshot,
  session,
  labels,
  onClose,
}: {
  atom: ActiveAtom;
  snapshot: SessionSnapshot;
  session: DocumentEditSession;
  labels: DocumentModeLabels;
  onClose: () => void;
}) {
  const found = findBlock(snapshot.state.doc, atom.blockId);
  const nodes = found ? getInlines(found.node, atom.part) : null;
  const node = nodes ? atomAt(nodes, atom.offset) : undefined;
  if (!node) return null;
  const attrs = node.attrs ?? {};
  const update = (patch: Record<string, unknown>) =>
    session.updateInlineAtom(atom.blockId, atom.part, atom.offset, patch);

  return (
    <fieldset
      aria-label={labels.inlineAtomEditor}
      className="m-0 flex min-w-0 flex-wrap items-center gap-2 border-0 border-b border-border bg-muted/40 px-3 py-1.5"
    >
      {node.type === "inlineMath" && (
        <>
          <label className="flex items-center gap-1 text-xs">
            {labels.mathSource}
            <input
              type="text"
              value={String(attrs.tex ?? "")}
              className={`${inputClass} w-64 font-mono`}
              onChange={(event) => update({ tex: event.target.value })}
            />
          </label>
          <label className="flex items-center gap-1 text-xs">
            <input
              type="checkbox"
              checked={attrs.display === true}
              onChange={(event) => update({ display: event.target.checked })}
            />
            {labels.mathDisplay}
          </label>
        </>
      )}
      {node.type === "rawInline" && (
        <>
          <span className="text-xs">
            {fillLabel(labels.rawBlock, { format: String(attrs.format ?? "") })}
          </span>
          <input
            type="text"
            aria-label={fillLabel(labels.rawBlock, { format: String(attrs.format ?? "") })}
            value={String(attrs.text ?? "")}
            className={`${inputClass} w-64 font-mono`}
            onChange={(event) => update({ text: event.target.value })}
          />
        </>
      )}
      {node.type === "footnoteRef" && (
        <span className="text-xs">
          {fillLabel(labels.footnoteReference, { id: String(attrs.noteId ?? "") })}
        </span>
      )}
      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        aria-label={labels.close}
        onClick={onClose}
      >
        <XIcon aria-hidden="true" />
      </Button>
    </fieldset>
  );
}
