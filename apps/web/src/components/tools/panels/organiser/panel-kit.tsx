"use client";
// Small building blocks and the parameter-emission hook shared by the Phase 15 custom panels.
import { type ReactNode, useEffect, useId, useMemo, useRef } from "react";
import { type ParamsValidation, validateParams } from "@/components/tools/params-form";
import type { ToolDescriptor } from "@/converter/tool-catalog";

export const controlClass =
  "h-9 min-w-0 rounded-lg border border-input bg-background px-2.5 text-sm text-foreground outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20";

/**
 * Validates the draft against the tool's JSON Schema plus the panel's own cross-field checks,
 * and reports the result whenever it changes. Returns the validation for inline error text.
 */
export function useEmitParams(
  tool: Pick<ToolDescriptor, "params_schema">,
  params: Record<string, unknown>,
  options: {
    pageCount?: number;
    extraErrors?: Record<string, string | undefined>;
    onChange: (params: Record<string, unknown>, validation: ParamsValidation) => void;
  },
): ParamsValidation {
  const { pageCount, extraErrors, onChange } = options;
  const extraKey = JSON.stringify(extraErrors ?? {});
  const validation = useMemo<ParamsValidation>(() => {
    const checked = validateParams(tool.params_schema, params, pageCount);
    const errors = { ...checked.errors, ...(JSON.parse(extraKey) as Record<string, string>) };
    return { isValid: Object.keys(errors).length === 0, errors };
  }, [tool.params_schema, params, pageCount, extraKey]);
  const onChangeRef = useRef(onChange);
  useEffect(() => {
    onChangeRef.current = onChange;
  }, [onChange]);
  const last = useRef<string | null>(null);
  useEffect(() => {
    const emission = JSON.stringify({ params, validation });
    if (last.current === emission) return;
    last.current = emission;
    onChangeRef.current(params, validation);
  }, [params, validation]);
  return validation;
}

export function ErrorText({ id, children }: { id?: string; children?: string }) {
  if (!children) return null;
  return (
    <p id={id} role="alert" className="text-sm text-destructive">
      {children}
    </p>
  );
}

export function Field({
  label,
  hint,
  error,
  children,
}: {
  label: string;
  hint?: string;
  error?: string;
  /** Receives the id to put on the control and the id of the error paragraph. */
  children: (ids: { id: string; errorId: string; invalid: boolean }) => ReactNode;
}) {
  const id = useId();
  return (
    <div className="grid gap-1.5">
      <label htmlFor={id} className="text-sm font-medium text-start">
        {label}
      </label>
      {children({ id, errorId: `${id}-error`, invalid: !!error })}
      {hint && !error ? <p className="text-xs text-muted-foreground text-start">{hint}</p> : null}
      <ErrorText id={`${id}-error`}>{error}</ErrorText>
    </div>
  );
}

export interface ChipOption<T extends string | number> {
  value: T;
  label: ReactNode;
  /** Plain-text name when `label` is not text. */
  name?: string;
}

/** A row of toggle chips that behaves as a single choice. */
export function ChipGroup<T extends string | number>({
  label,
  options,
  value,
  onSelect,
  disabled,
}: {
  label: string;
  options: readonly ChipOption<T>[];
  value: T | null;
  onSelect: (value: T) => void;
  disabled?: boolean;
}) {
  return (
    // biome-ignore lint/a11y/useSemanticElements: a labelled group of toggle buttons
    <div role="group" aria-label={label} className="flex flex-wrap gap-1.5">
      {options.map((option) => (
        <button
          key={String(option.value)}
          type="button"
          disabled={disabled}
          aria-pressed={value === option.value}
          aria-label={option.name}
          onClick={() => onSelect(option.value)}
          className="inline-flex h-8 items-center gap-1.5 rounded-full border border-input bg-background px-3 text-sm outline-none transition-colors hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 aria-pressed:border-primary aria-pressed:bg-primary aria-pressed:text-primary-foreground"
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

/** A miniature page: a number on a paper-coloured tile. */
export function PagePill({
  label,
  tone = "page",
  source,
}: {
  label: ReactNode;
  tone?: "page" | "blank" | "a" | "b";
  /** Original page this tile stands for, for assistive tech and tests. */
  source?: string;
}) {
  const tones = {
    page: "border-border bg-background text-foreground",
    blank: "border-dashed border-muted-foreground/50 bg-muted text-muted-foreground",
    a: "border-primary/40 bg-primary/10 text-foreground",
    b: "border-accent-foreground/30 bg-accent text-accent-foreground",
  } as const;
  return (
    <span
      data-slot="page-pill"
      data-tone={tone}
      data-source={source}
      className={`inline-flex h-9 min-w-8 items-center justify-center rounded-sm border px-1.5 text-xs font-medium tabular-nums ${tones[tone]}`}
    >
      {label}
    </span>
  );
}

/** Active page count: the explicit prop, else the first file's known count. */
export function resolvePageCount(
  pageCount: number | undefined,
  files: ReadonlyArray<{ pageCount: number | null }>,
): number | undefined {
  return pageCount ?? files[0]?.pageCount ?? undefined;
}
