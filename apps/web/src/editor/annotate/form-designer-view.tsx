"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { ArrowDownIcon, ArrowUpIcon, PlusIcon, Trash2Icon } from "lucide-react";
import { useStore } from "zustand";
import type { PageSize } from "./annotation-canvas";
import { useAnnotateCopy } from "./copy";
import {
  DESIGN_FIELD_TYPES,
  type DesignField,
  designIssues,
  type FormDesignStore,
} from "./form-model";

/** Vertical gap between fields added one after another, in points. */
const STACK_STEP = 36;
const TOP_MARGIN = 72;

export interface FormDesignerViewProps {
  store: FormDesignStore;
  /** Page the next added field lands on. */
  page: number;
  pageCount: number;
  pageSize: PageSize;
}

/**
 * Designs a new AcroForm: add fields, edit their name, default, required flag, choices and box,
 * and order them. The list order is the tab order and each row carries its number.
 */
export function FormDesignerView({ store, page, pageCount, pageSize }: FormDesignerViewProps) {
  const copy = useAnnotateCopy().design;
  const state = useStore(store);
  const issues = designIssues(state.fields);
  const selected = state.fields.find((field) => field.key === state.selectedKey) ?? null;

  function addField(type: (typeof DESIGN_FIELD_TYPES)[number]) {
    const onPage = state.fields.filter((field) => field.page === page).length;
    const top = Math.max(STACK_STEP, pageSize.height - TOP_MARGIN - onPage * STACK_STEP);
    state.add(type, page, top);
  }

  return (
    <section
      data-slot="form-designer"
      aria-label={copy.title}
      className="flex min-w-0 flex-col gap-3"
    >
      <fieldset
        aria-label={copy.add}
        className="m-0 min-w-0 border-0 p-0 flex flex-wrap items-center gap-1.5"
      >
        {DESIGN_FIELD_TYPES.map((type) => (
          <Button
            key={type}
            type="button"
            size="sm"
            variant="outline"
            aria-label={copy.addType(copy.types[type])}
            onClick={() => addField(type)}
          >
            <PlusIcon aria-hidden="true" />
            {copy.types[type]}
          </Button>
        ))}
        <Badge variant="secondary">{copy.fieldCount(state.fields.length)}</Badge>
      </fieldset>

      {state.fields.length === 0 ? (
        <p className="text-sm text-muted-foreground">{copy.empty}</p>
      ) : (
        <ol aria-label={copy.list} className="flex flex-col gap-1">
          {state.fields.map((field, index) => (
            <li
              key={field.key}
              data-field-key={field.key}
              data-invalid={issues.has(field.key) ? "true" : undefined}
              className="flex items-center gap-2 rounded-md border px-2 py-1 text-sm"
            >
              <Badge
                variant="outline"
                aria-label={copy.tabOrder(index + 1)}
                data-tab-order={index + 1}
              >
                {index + 1}
              </Badge>
              <button
                type="button"
                aria-pressed={state.selectedKey === field.key}
                aria-label={copy.select(field.name)}
                className="min-w-0 flex-1 truncate text-start outline-none focus-visible:underline aria-pressed:font-medium"
                onClick={() => state.select(field.key)}
              >
                {field.name}
                <span className="ms-2 text-xs text-muted-foreground">{copy.types[field.type]}</span>
                {field.required ? <span className="ms-1 text-destructive">*</span> : null}
              </button>
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-label={copy.moveUp(field.name)}
                disabled={index === 0}
                onClick={() => state.move(field.key, -1)}
              >
                <ArrowUpIcon aria-hidden="true" />
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-label={copy.moveDown(field.name)}
                disabled={index === state.fields.length - 1}
                onClick={() => state.move(field.key, 1)}
              >
                <ArrowDownIcon aria-hidden="true" />
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-label={copy.remove(field.name)}
                onClick={() => state.remove(field.key)}
              >
                <Trash2Icon aria-hidden="true" />
              </Button>
            </li>
          ))}
        </ol>
      )}

      {selected ? (
        <FieldEditor
          field={selected}
          store={store}
          pageCount={pageCount}
          problems={issues.get(selected.key) ?? []}
        />
      ) : null}
    </section>
  );
}

const input = "h-7 w-full rounded-md border bg-background px-2 text-sm";

function FieldEditor({
  field,
  store,
  pageCount,
  problems,
}: {
  field: DesignField;
  store: FormDesignStore;
  pageCount: number;
  problems: readonly (keyof ReturnType<typeof useAnnotateCopy>["design"]["issues"])[];
}) {
  const copy = useAnnotateCopy().design;
  const { update } = useStore(store);
  const setRect = (position: 0 | 1 | 2 | 3, text: string) => {
    const rect = [...field.rect] as [number, number, number, number];
    rect[position] = text === "" ? Number.NaN : Number(text);
    update(field.key, { rect });
  };

  return (
    <fieldset
      aria-label={copy.select(field.name)}
      className="flex flex-col gap-2 rounded-lg border p-2"
    >
      <legend className="px-1 text-xs text-muted-foreground">{copy.types[field.type]}</legend>
      <label className="flex flex-col gap-1 text-xs">
        {copy.name}
        <input
          type="text"
          value={field.name}
          aria-invalid={problems.some((problem) => problem.startsWith("name")) || undefined}
          onChange={(event) => update(field.key, { name: event.currentTarget.value })}
          className={input}
        />
      </label>
      <label className="flex flex-col gap-1 text-xs">
        {copy.label}
        <input
          type="text"
          value={field.label}
          onChange={(event) => update(field.key, { label: event.currentTarget.value })}
          className={input}
        />
      </label>
      {field.type === "checkbox" ? (
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={field.checked}
            onChange={(event) => update(field.key, { checked: event.currentTarget.checked })}
          />
          {copy.checked}
        </label>
      ) : null}
      {field.type === "text" || field.type === "dropdown" ? (
        <label className="flex flex-col gap-1 text-xs">
          {copy.defaultValue}
          <input
            type="text"
            value={field.defaultText}
            aria-invalid={problems.includes("default") || undefined}
            onChange={(event) => update(field.key, { defaultText: event.currentTarget.value })}
            className={input}
          />
        </label>
      ) : null}
      {field.type === "text" ? (
        <label className="flex items-center gap-2 text-sm">
          <input
            type="checkbox"
            checked={field.multiline}
            onChange={(event) => update(field.key, { multiline: event.currentTarget.checked })}
          />
          {copy.multiline}
        </label>
      ) : null}
      {field.type === "dropdown" ? (
        <label className="flex flex-col gap-1 text-xs">
          {copy.options}
          <textarea
            rows={3}
            value={field.options}
            onChange={(event) => update(field.key, { options: event.currentTarget.value })}
            className="rounded-md border bg-background p-1.5 text-sm"
          />
        </label>
      ) : null}
      <label className="flex items-center gap-2 text-sm">
        <input
          type="checkbox"
          checked={field.required}
          onChange={(event) => update(field.key, { required: event.currentTarget.checked })}
        />
        {copy.required}
      </label>
      <label className="flex flex-col gap-1 text-xs">
        {copy.page}
        <input
          type="number"
          min={1}
          max={pageCount}
          value={field.page}
          onChange={(event) =>
            update(field.key, {
              page: Math.min(
                pageCount,
                Math.max(1, Math.trunc(Number(event.currentTarget.value)) || 1),
              ),
            })
          }
          className={input}
        />
      </label>
      <fieldset className="grid grid-cols-2 gap-2" aria-label={copy.box} dir="ltr">
        {(
          [
            [copy.left, 0],
            [copy.bottom, 1],
            [copy.right, 2],
            [copy.top, 3],
          ] as const
        ).map(([label, position]) => (
          <label key={position} className="flex flex-col gap-1 text-xs">
            {label}
            <input
              type="number"
              value={Number.isFinite(field.rect[position]) ? field.rect[position] : ""}
              aria-invalid={problems.includes("rect") || undefined}
              onChange={(event) => setRect(position, event.currentTarget.value)}
              className={input}
            />
          </label>
        ))}
      </fieldset>
      {problems.length > 0 ? (
        <ul role="alert" className="flex flex-col gap-0.5 text-xs text-destructive">
          {problems.map((problem) => (
            <li key={problem}>{copy.issues[problem]}</li>
          ))}
        </ul>
      ) : null}
    </fieldset>
  );
}

/** A passive preview of the designed fields on a page, in the page's point units. */
export function FormDesignOverlay({
  store,
  page,
  size,
}: {
  store: FormDesignStore;
  page: number;
  size: PageSize;
}) {
  const state = useStore(store);
  return (
    <div data-slot="design-overlay" dir="ltr" className="pointer-events-none absolute inset-0">
      {state.fields.map((field, index) => {
        if (field.page !== page) return null;
        const [left, bottom, right, top] = field.rect;
        if (![left, bottom, right, top].every(Number.isFinite)) return null;
        return (
          <div
            key={field.key}
            data-tab-order={index + 1}
            className={`absolute rounded-sm border-2 bg-primary/10 text-[10px] font-semibold ${
              state.selectedKey === field.key ? "border-primary" : "border-primary/50"
            }`}
            style={{
              left: `${(left / size.width) * 100}%`,
              top: `${((size.height - top) / size.height) * 100}%`,
              width: `${((right - left) / size.width) * 100}%`,
              height: `${((top - bottom) / size.height) * 100}%`,
            }}
          >
            <span className="absolute -top-3 -left-0.5 rounded bg-primary px-1 text-primary-foreground">
              {index + 1}
            </span>
          </div>
        );
      })}
    </div>
  );
}
