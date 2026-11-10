"use client";

import { controlClass, Field } from "../ocr/panel-kit";
import { signCopy } from "./copy";

export interface PlacementDraft {
  page: string;
  x1: string;
  y1: string;
  x2: string;
  y2: string;
}

export const DEFAULT_PLACEMENT: PlacementDraft = {
  page: "1",
  x1: "72",
  y1: "72",
  x2: "252",
  y2: "132",
};

export interface Placement {
  page: number;
  rect: [number, number, number, number];
}

const A4 = { width: 595, height: 842 };
const keys = ["x1", "y1", "x2", "y2"] as const;

/** A placement draft turned into numbers, with the first reason it is not valid yet. */
export function parsePlacement(
  draft: PlacementDraft,
  options: { pageCount?: number; wholeNumbers?: boolean },
): { placement?: Placement; errors: { page?: string; rect?: string } } {
  const copy = signCopy.placement;
  const errors: { page?: string; rect?: string } = {};
  const page = Number(draft.page);
  if (draft.page.trim() === "" || !Number.isInteger(page) || page < 1)
    errors.page = copy.pageInvalid;
  else if (options.pageCount !== undefined && page > options.pageCount)
    errors.page = copy.pageRange(options.pageCount);
  const raw = keys.map((key) => draft[key]);
  const rect = raw.map(Number) as Placement["rect"];
  if (raw.some((value) => value.trim() === "") || !rect.every(Number.isFinite))
    errors.rect = copy.numbers;
  else if (options.wholeNumbers && !rect.every(Number.isInteger)) errors.rect = copy.wholeNumbers;
  else if (rect[0] >= rect[2] || rect[1] >= rect[3]) errors.rect = copy.order;
  if (errors.page || errors.rect) return { errors };
  return { placement: { page, rect: rect }, errors };
}

/** Page, box coordinates and a small A4 sketch showing where the signature box lands. */
export function PlacementFields({
  draft,
  onChange,
  errors,
  placement,
  disabled,
}: {
  draft: PlacementDraft;
  onChange: (draft: PlacementDraft) => void;
  errors: { page?: string; rect?: string };
  placement?: Placement;
  disabled?: boolean;
}) {
  const copy = signCopy.placement;
  const box = placement?.rect;
  return (
    <fieldset className="grid gap-3 border-0 p-0" disabled={disabled} data-slot="sign-placement">
      <legend className="text-sm font-semibold text-start">{copy.title}</legend>
      <div className="grid gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,9rem)]">
        <div className="grid gap-3">
          <Field label={copy.page} error={errors.page}>
            {({ id, invalid: _invalid, ...aria }) => (
              <input
                {...aria}
                id={id}
                type="number"
                inputMode="numeric"
                min={1}
                step={1}
                className={`${controlClass} w-full`}
                value={draft.page}
                onChange={(event) => onChange({ ...draft, page: event.target.value })}
              />
            )}
          </Field>
          <div className="grid grid-cols-2 gap-3">
            {keys.map((key) => (
              <Field key={key} label={copy[key]} error={undefined}>
                {({ id, invalid: _invalid, ...aria }) => (
                  <input
                    {...aria}
                    id={id}
                    type="number"
                    inputMode="decimal"
                    step="any"
                    className={`${controlClass} w-full`}
                    value={draft[key]}
                    onChange={(event) => onChange({ ...draft, [key]: event.target.value })}
                  />
                )}
              </Field>
            ))}
          </div>
          {errors.rect ? (
            <p role="alert" className="text-sm text-destructive text-start">
              {errors.rect}
            </p>
          ) : (
            <p className="text-xs text-muted-foreground text-start">{copy.hint}</p>
          )}
        </div>
        <div
          role="img"
          aria-label={copy.preview}
          className="relative mx-auto aspect-[595/842] w-full max-w-36 rounded-sm border border-input bg-card shadow-elevation-1"
        >
          {box ? (
            <div
              data-slot="sign-placement-box"
              className="absolute rounded-xs border-2 border-primary bg-primary/15"
              style={{
                insetInlineStart: `${Math.min(100, Math.max(0, (box[0] / A4.width) * 100))}%`,
                bottom: `${Math.min(100, Math.max(0, (box[1] / A4.height) * 100))}%`,
                width: `${Math.min(100, ((box[2] - box[0]) / A4.width) * 100)}%`,
                height: `${Math.min(100, ((box[3] - box[1]) / A4.height) * 100)}%`,
              }}
            />
          ) : null}
        </div>
      </div>
    </fieldset>
  );
}
