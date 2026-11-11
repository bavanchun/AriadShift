"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { RotateCcwIcon, Trash2Icon } from "lucide-react";
import { useStore } from "zustand";
import { type Annotation, type AnnotationStore, pendingCount } from "./annotation-store";
import { annotationLabel, useAnnotateCopy } from "./copy";

/** The pending annotations, the ones already in the PDF, and an editor for the selection. */
export function AnnotationList({ store }: { store: AnnotationStore }) {
  const copy = useAnnotateCopy();
  const state = useStore(store);
  const selected = state.items.find((item) => item.id === state.selectedId) ?? null;

  return (
    <section
      data-slot="annotation-list"
      aria-label={copy.list.title}
      className="flex flex-col gap-3"
    >
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-sm font-medium">{copy.list.title}</h3>
        <Badge variant="secondary">{copy.list.pending(pendingCount(state))}</Badge>
      </div>

      {state.items.length === 0 && state.existing.length === 0 ? (
        <p className="text-sm text-muted-foreground">{copy.list.empty}</p>
      ) : (
        <ul className="flex flex-col gap-1">
          {state.items.map((item, index) => {
            const label = annotationLabel(copy, item, index);
            return (
              <li
                key={item.id}
                data-annotation-id={item.id}
                className="flex items-center justify-between gap-2 rounded-md border px-2 py-1 text-sm"
              >
                <button
                  type="button"
                  aria-pressed={state.selectedId === item.id}
                  className="min-w-0 flex-1 truncate text-start outline-none focus-visible:underline aria-pressed:font-medium"
                  onClick={() => state.select(item.id)}
                >
                  {copy.list.item(label, item.page)}
                </button>
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-xs"
                  aria-label={copy.list.remove(label)}
                  onClick={() => state.remove(item.id)}
                >
                  <Trash2Icon aria-hidden="true" />
                </Button>
              </li>
            );
          })}
          {state.existing.map((entry) => {
            const gone = state.removed.includes(entry.id);
            const label = copy.list.existing(entry.type, entry.page);
            return (
              <li
                key={entry.id}
                data-existing-id={entry.id}
                data-removed={gone ? "true" : undefined}
                className="flex items-center justify-between gap-2 rounded-md border border-dashed px-2 py-1 text-sm"
              >
                <span
                  className={`min-w-0 flex-1 truncate ${gone ? "line-through opacity-60" : ""}`}
                >
                  {label}
                </span>
                {gone ? <Badge variant="destructive">{copy.list.removed}</Badge> : null}
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-xs"
                  aria-label={gone ? copy.list.restore(label) : copy.list.remove(label)}
                  onClick={() => (gone ? state.restoreExisting(entry.id) : state.remove(entry.id))}
                >
                  {gone ? <RotateCcwIcon aria-hidden="true" /> : <Trash2Icon aria-hidden="true" />}
                </Button>
              </li>
            );
          })}
        </ul>
      )}

      {selected ? <AnnotationInspector annotation={selected} store={store} /> : null}
    </section>
  );
}

function AnnotationInspector({
  annotation,
  store,
}: {
  annotation: Annotation;
  store: AnnotationStore;
}) {
  const copy = useAnnotateCopy();
  const state = useStore(store);
  const label = annotationLabel(copy, annotation, state.items.indexOf(annotation));

  return (
    <fieldset aria-label={label} className="flex flex-col gap-2 rounded-lg border p-2">
      <legend className="px-1 text-xs text-muted-foreground">{label}</legend>
      <label className="flex items-center gap-2 text-xs">
        {copy.toolbar.colour}
        <input
          type="color"
          value={annotation.color}
          aria-label={copy.toolbar.colour}
          onChange={(event) => state.update(annotation.id, { color: event.currentTarget.value })}
          className="size-6 cursor-pointer rounded border bg-transparent p-0"
        />
      </label>
      <label className="flex items-center gap-2 text-xs">
        {copy.toolbar.opacity}
        <input
          type="range"
          min={0.1}
          max={1}
          step={0.05}
          value={annotation.opacity}
          aria-label={copy.toolbar.opacity}
          onChange={(event) =>
            state.update(annotation.id, { opacity: Number(event.currentTarget.value) })
          }
        />
      </label>
      {annotation.kind === "ink" || annotation.kind === "shape" ? (
        <label className="flex items-center gap-2 text-xs">
          {copy.toolbar.strokeWidth}
          <input
            type="range"
            min={1}
            max={12}
            step={1}
            value={annotation.lineWidth}
            aria-label={copy.toolbar.strokeWidth}
            onChange={(event) =>
              state.update(annotation.id, { lineWidth: Number(event.currentTarget.value) })
            }
          />
        </label>
      ) : null}
      {annotation.kind === "note" ? (
        <>
          <label className="flex flex-col gap-1 text-xs">
            {copy.list.comment}
            <textarea
              value={annotation.contents}
              rows={3}
              onChange={(event) =>
                state.update(annotation.id, { contents: event.currentTarget.value })
              }
              className="rounded-md border bg-background p-1.5 text-sm"
            />
          </label>
          <label className="flex flex-col gap-1 text-xs">
            {copy.toolbar.author}
            <input
              type="text"
              value={annotation.author}
              onChange={(event) =>
                state.update(annotation.id, { author: event.currentTarget.value })
              }
              className="h-7 rounded-md border bg-background px-2 text-sm"
            />
          </label>
        </>
      ) : null}
      {annotation.kind === "stamp" ? (
        <label className="flex flex-col gap-1 text-xs">
          {copy.list.stampText}
          <input
            type="text"
            value={annotation.label}
            onChange={(event) => state.update(annotation.id, { label: event.currentTarget.value })}
            className="h-7 rounded-md border bg-background px-2 text-sm"
          />
        </label>
      ) : null}
      <p className="text-xs text-muted-foreground">{copy.list.nudge}</p>
      <Button
        type="button"
        variant="destructive"
        size="sm"
        onClick={() => state.remove(annotation.id)}
      >
        <Trash2Icon aria-hidden="true" />
        {copy.list.delete}
      </Button>
    </fieldset>
  );
}
