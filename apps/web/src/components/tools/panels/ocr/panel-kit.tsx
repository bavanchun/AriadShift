"use client";
// Props contract and small building blocks shared by the OCR, redact, sign and compare panels.
import { type ReactNode, useEffect, useId, useMemo, useRef } from "react";
import { type ParamsValidation, validateParams } from "@/components/tools/params-form";
import type { WorkspaceFile } from "@/components/workspace/types";
import type { ToolDescriptor } from "@/converter/tool-catalog";

/**
 * Props every purpose-built tool panel receives. A panel owns its draft parameters and reports
 * each change, with the validation the generic form would have produced, through `onChange`.
 */
export interface CustomToolPanelProps {
  tool: ToolDescriptor;
  /** The files the tool will read, in the order the workspace passes them to the engine. */
  files: WorkspaceFile[];
  /** Page count of the file the tool reads, for page-range validation. */
  pageCount?: number;
  disabled?: boolean;
  locale?: string;
  onChange: (params: Record<string, unknown>, validation: ParamsValidation) => void;
}

export const controlClass =
  "h-9 min-w-0 rounded-lg border border-input bg-background px-2.5 text-sm text-foreground outline-none transition-colors placeholder:text-muted-foreground focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 aria-invalid:border-destructive aria-invalid:ring-3 aria-invalid:ring-destructive/20";

/**
 * Validates the draft against the tool's JSON Schema plus the panel's own cross-field checks and
 * reports the result whenever it changes. `schemaParams` lets a panel validate a stand-in for a
 * value that must not be run through the schema (a `$secret` reference, for example).
 */
export function useEmitParams(
  tool: Pick<ToolDescriptor, "params_schema">,
  params: Record<string, unknown>,
  options: {
    pageCount?: number;
    extraErrors?: Record<string, string>;
    schemaParams?: Record<string, unknown>;
    onChange: (params: Record<string, unknown>, validation: ParamsValidation) => void;
  },
): ParamsValidation {
  const { pageCount, extraErrors, schemaParams, onChange } = options;
  const extraKey = JSON.stringify(extraErrors ?? {});
  const validation = useMemo<ParamsValidation>(() => {
    const checked = validateParams(tool.params_schema, schemaParams ?? params, pageCount);
    const errors = { ...checked.errors, ...(JSON.parse(extraKey) as Record<string, string>) };
    return { isValid: Object.keys(errors).length === 0, errors };
  }, [tool.params_schema, schemaParams, params, pageCount, extraKey]);
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

/** A numeric text draft: empty means "use the tool default", anything else must be a number. */
export function numberDraft(draft: string): { value?: number; error?: string } {
  const text = draft.trim();
  if (text === "") return {};
  const value = Number(text);
  return Number.isFinite(value) ? { value } : { error: "Enter a number." };
}

export function ErrorText({ id, children }: { id?: string; children?: string }) {
  if (!children) return null;
  return (
    <p id={id} role="alert" className="text-sm text-destructive text-start">
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
  /** Receives the id to put on the control and the props that wire up the error text. */
  children: (ids: {
    id: string;
    invalid: boolean;
    "aria-invalid": boolean;
    "aria-describedby": string | undefined;
  }) => ReactNode;
}) {
  const id = useId();
  const errorId = `${id}-error`;
  return (
    <div className="grid gap-1.5">
      <label htmlFor={id} className="text-sm font-medium text-start">
        {label}
      </label>
      {children({
        id,
        invalid: !!error,
        "aria-invalid": !!error,
        "aria-describedby": error ? errorId : undefined,
      })}
      {hint && !error ? <p className="text-xs text-muted-foreground text-start">{hint}</p> : null}
      <ErrorText id={errorId}>{error}</ErrorText>
    </div>
  );
}

/** A labelled checkbox row with an optional explanation underneath. */
export function CheckRow({
  label,
  hint,
  checked,
  onChange,
  disabled,
}: {
  label: string;
  hint?: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
}) {
  return (
    <label className="flex cursor-pointer items-start gap-2.5 rounded-lg border border-input bg-background p-2.5 text-start has-disabled:cursor-not-allowed has-disabled:opacity-50">
      <input
        type="checkbox"
        className="mt-0.5 size-4 shrink-0 cursor-pointer rounded-sm border border-input accent-primary outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onChange(event.target.checked)}
      />
      <span className="grid gap-0.5">
        <span className="text-sm font-medium">{label}</span>
        {hint ? <span className="text-xs text-muted-foreground">{hint}</span> : null}
      </span>
    </label>
  );
}

export interface ChipOption<T extends string> {
  value: T;
  label: string;
}

/** A row of toggle chips behaving as a single choice. */
export function ChoiceChips<T extends string>({
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
        <Chip
          key={option.value}
          pressed={value === option.value}
          disabled={disabled}
          onClick={() => onSelect(option.value)}
        >
          {option.label}
        </Chip>
      ))}
    </div>
  );
}

export function Chip({
  pressed,
  onClick,
  disabled,
  children,
  label,
}: {
  pressed: boolean;
  onClick: () => void;
  disabled?: boolean;
  children: ReactNode;
  label?: string;
}) {
  return (
    <button
      type="button"
      disabled={disabled}
      aria-pressed={pressed}
      aria-label={label}
      onClick={onClick}
      className="inline-flex h-8 items-center gap-1.5 rounded-full border border-input bg-background px-3 text-sm outline-none transition-colors hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50 aria-pressed:border-primary aria-pressed:bg-primary aria-pressed:text-primary-foreground"
    >
      {children}
    </button>
  );
}

export const sectionTitleClass = "text-sm font-semibold text-start";
