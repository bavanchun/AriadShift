"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { useId, useRef } from "react";
import { useStore } from "zustand";
import type { PageSize } from "./annotation-canvas";
import { useAnnotateCopy } from "./copy";
import {
  choicesOf,
  emptyRequiredFields,
  type FormFieldInfo,
  type FormFillStore,
  fillKind,
  isFillable,
} from "./form-model";
import type { FormValue } from "./tool-params";

const controlId = (prefix: string, index: number) => `${prefix}-field-${index}`;

const fieldLabel = (field: FormFieldInfo) =>
  field.tooltip && field.tooltip.trim() !== "" ? field.tooltip : field.name;

const asText = (value: FormValue | undefined) =>
  typeof value === "string" ? value : Array.isArray(value) ? (value[0] ?? "") : "";

const isChecked = (value: FormValue | undefined) =>
  value === true || (typeof value === "string" && value !== "" && value !== "Off");

export interface FormFillerViewProps {
  store: FormFillStore;
  /** Prefix that keeps control ids unique when several views share a page. */
  idPrefix?: string;
}

/**
 * The structured side list of a form: one labelled control per field in tab order, a badge
 * counting the required fields still empty, and the flatten option.
 */
export function FormFillerView({ store, idPrefix }: FormFillerViewProps) {
  const copy = useAnnotateCopy().fill;
  const generated = useId();
  const prefix = idPrefix ?? generated;
  const state = useStore(store);
  const lastFocused = useRef(-1);
  const remaining = emptyRequiredFields(state.fields, state.values);

  function goToNextRequired() {
    const indexes = remaining
      .map((name) => state.fields.findIndex((field) => field.name === name))
      .filter((index) => index >= 0);
    const next = indexes.find((index) => index > lastFocused.current) ?? indexes[0];
    if (next !== undefined) document.getElementById(controlId(prefix, next))?.focus();
  }

  if (state.fields.length === 0) {
    return (
      <section data-slot="form-filler" aria-label={copy.title}>
        <p className="text-sm text-muted-foreground">{copy.empty}</p>
      </section>
    );
  }

  return (
    <section
      data-slot="form-filler"
      aria-label={copy.title}
      className="flex min-w-0 flex-col gap-3"
    >
      <div className="flex flex-wrap items-center gap-2">
        <Badge role="status" variant={remaining.length > 0 ? "destructive" : "secondary"}>
          {copy.remaining(remaining.length)}
        </Badge>
        <Button
          type="button"
          size="xs"
          variant="outline"
          disabled={remaining.length === 0}
          onClick={goToNextRequired}
        >
          {copy.next}
        </Button>
        <Button type="button" size="xs" variant="ghost" onClick={state.reset}>
          {copy.reset}
        </Button>
      </div>

      <ol aria-label={copy.list} className="flex flex-col gap-2">
        {state.fields.map((field, index) => {
          const kind = fillKind(field);
          const id = controlId(prefix, index);
          const empty = remaining.includes(field.name);
          const label = fieldLabel(field);
          const value = state.values[field.name];
          const disabled = !isFillable(field);
          const common = {
            id,
            disabled,
            "aria-required": field.flags?.required ? (true as const) : undefined,
            "aria-invalid": empty ? (true as const) : undefined,
            onFocus: () => {
              lastFocused.current = index;
            },
          };
          const fieldClass = `h-8 w-full rounded-md border bg-background px-2 text-sm ${
            empty ? "border-destructive" : ""
          }`;
          return (
            <li
              key={field.name}
              data-field={field.name}
              data-empty-required={empty ? "true" : undefined}
              className="flex flex-col gap-1 rounded-lg border p-2"
            >
              {kind === "radio" ? (
                <fieldset className="flex flex-col gap-1" disabled={disabled}>
                  <legend className="text-sm font-medium">
                    {label}
                    {field.flags?.required ? (
                      <span className="ms-1 text-destructive" title={copy.required}>
                        *
                      </span>
                    ) : null}
                  </legend>
                  {choicesOf(field).map((choice, position) => (
                    <label key={choice.export} className="flex items-center gap-2 text-sm">
                      <input
                        type="radio"
                        name={`${prefix}-${field.name}`}
                        id={position === 0 ? id : undefined}
                        checked={value === choice.export}
                        required={field.flags?.required === true}
                        onFocus={common.onFocus}
                        onChange={() => state.setValue(field.name, choice.export)}
                      />
                      {choice.display}
                    </label>
                  ))}
                </fieldset>
              ) : kind === "checkbox" ? (
                <label className="flex items-center gap-2 text-sm font-medium">
                  <input
                    type="checkbox"
                    {...common}
                    checked={isChecked(value)}
                    onChange={(event) => state.setValue(field.name, event.currentTarget.checked)}
                  />
                  {label}
                  {field.flags?.required ? (
                    <span className="text-destructive" title={copy.required}>
                      *
                    </span>
                  ) : null}
                </label>
              ) : (
                <>
                  <label htmlFor={id} className="text-sm font-medium">
                    {label}
                    {field.flags?.required ? (
                      <span className="ms-1 text-destructive" title={copy.required}>
                        *
                      </span>
                    ) : null}
                  </label>
                  {kind === "text" ? (
                    <input
                      type="text"
                      {...common}
                      value={asText(value)}
                      onChange={(event) => state.setValue(field.name, event.currentTarget.value)}
                      className={fieldClass}
                    />
                  ) : null}
                  {kind === "multiline" ? (
                    <textarea
                      {...common}
                      rows={3}
                      value={asText(value)}
                      onChange={(event) => state.setValue(field.name, event.currentTarget.value)}
                      className={`${fieldClass} h-auto py-1`}
                    />
                  ) : null}
                  {kind === "select" ? (
                    <select
                      {...common}
                      multiple={field.flags?.multi_select === true}
                      value={
                        field.flags?.multi_select
                          ? Array.isArray(value)
                            ? value
                            : []
                          : asText(value)
                      }
                      onChange={(event) =>
                        state.setValue(
                          field.name,
                          field.flags?.multi_select
                            ? Array.from(event.currentTarget.selectedOptions, (o) => o.value)
                            : event.currentTarget.value === ""
                              ? null
                              : event.currentTarget.value,
                        )
                      }
                      className={fieldClass}
                    >
                      {field.flags?.multi_select ? null : <option value="">{copy.choose}</option>}
                      {choicesOf(field).map((choice) => (
                        <option key={choice.export} value={choice.export}>
                          {choice.display}
                        </option>
                      ))}
                    </select>
                  ) : null}
                  {kind === "signature" ? (
                    <p id={id} className="text-xs text-muted-foreground">
                      {copy.signature}
                    </p>
                  ) : null}
                </>
              )}
              <span className="text-xs text-muted-foreground">
                {copy.page(field.page)}
                {field.flags?.read_only ? ` · ${copy.readOnly}` : ""}
              </span>
            </li>
          );
        })}
      </ol>

      <label className="flex items-start gap-2 text-sm">
        <input
          type="checkbox"
          checked={state.flatten}
          onChange={(event) => state.setFlatten(event.currentTarget.checked)}
          className="mt-0.5"
        />
        <span className="flex flex-col">
          {copy.flatten}
          <span className="text-xs text-muted-foreground">{copy.flattenHint}</span>
        </span>
      </label>
    </section>
  );
}

export interface FormFieldOverlayProps {
  store: FormFillStore;
  page: number;
  size: PageSize;
  idPrefix: string;
}

/**
 * Marks every field of a page on top of the page view. Each marker is a pointer shortcut to the
 * field's control in the side list; keyboard users reach the same control in tab order there.
 */
export function FormFieldOverlay({ store, page, size, idPrefix }: FormFieldOverlayProps) {
  const copy = useAnnotateCopy().fill;
  const state = useStore(store);
  const remaining = emptyRequiredFields(state.fields, state.values);
  return (
    <div data-slot="form-overlay" dir="ltr" className="pointer-events-none absolute inset-0">
      {state.fields.map((field, index) => {
        if (field.page !== page) return null;
        const [left, bottom, right, top] = field.rect;
        const empty = remaining.includes(field.name);
        return (
          <button
            key={field.name}
            type="button"
            tabIndex={-1}
            data-empty-required={empty ? "true" : undefined}
            aria-label={copy.marker(fieldLabel(field), index + 1)}
            onClick={() => document.getElementById(controlId(idPrefix, index))?.focus()}
            className={`pointer-events-auto absolute rounded-sm border-2 bg-primary/10 ${
              empty ? "border-destructive bg-destructive/10" : "border-primary/60"
            }`}
            style={{
              left: `${(left / size.width) * 100}%`,
              top: `${((size.height - top) / size.height) * 100}%`,
              width: `${((right - left) / size.width) * 100}%`,
              height: `${((top - bottom) / size.height) * 100}%`,
            }}
          />
        );
      })}
    </div>
  );
}
