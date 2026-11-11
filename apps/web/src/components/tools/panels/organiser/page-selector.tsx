"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { useRef } from "react";
import { organiserCopy } from "./copy";
import { countSelectedPages } from "./page-range";
import { ChipGroup, controlClass, Field } from "./panel-kit";

const copy = organiserCopy.selector;
type Quick = "all" | "odd" | "even" | "first" | "last" | "custom";
const QUICK_VALUES: Record<Exclude<Quick, "custom">, string> = {
  all: "all",
  odd: "odd",
  even: "even",
  first: "1",
  last: "last",
};

function quickFor(value: string): Quick {
  const trimmed = value.trim();
  const found = (Object.keys(QUICK_VALUES) as Array<Exclude<Quick, "custom">>).find(
    (key) => QUICK_VALUES[key] === trimmed,
  );
  return found ?? "custom";
}

export interface PageSelectorProps {
  /** Label of the text input, naming what the selection is for. */
  label?: string;
  value: string;
  onChange: (value: string) => void;
  pageCount?: number;
  disabled?: boolean;
  error?: string;
  /** Hide the quick chips for a plain validated input (custom page order). */
  chips?: boolean;
  hint?: string;
}

/** Quick range chips, a live validated range input and the count of pages it selects. */
export function PageSelector({
  label = copy.input,
  value,
  onChange,
  pageCount,
  disabled,
  error,
  chips = true,
  hint = copy.hint,
}: PageSelectorProps) {
  const input = useRef<HTMLInputElement>(null);
  const selected = pageCount === undefined ? null : countSelectedPages(value, pageCount);
  const quickOptions = (["all", "odd", "even", "first", "last", "custom"] as const).map((key) => ({
    value: key,
    label: copy[key],
  }));
  return (
    <div data-slot="page-selector" className="grid gap-2">
      {chips ? (
        <ChipGroup<Quick>
          label={copy.group}
          options={quickOptions}
          value={quickFor(value)}
          disabled={disabled}
          onSelect={(key) => {
            if (key === "custom") input.current?.focus();
            else onChange(QUICK_VALUES[key]);
          }}
        />
      ) : null}
      <Field label={label} hint={hint} error={error}>
        {({ id, errorId, invalid }) => (
          <div className="flex flex-wrap items-center gap-2">
            <input
              ref={input}
              id={id}
              type="text"
              spellCheck={false}
              autoComplete="off"
              disabled={disabled}
              aria-invalid={invalid}
              aria-describedby={invalid ? errorId : undefined}
              className={`${controlClass} min-w-40 flex-1 text-start`}
              value={value}
              onChange={(event) => onChange(event.target.value)}
            />
            {selected !== null && !invalid ? (
              <Badge variant="secondary" data-slot="selection-count">
                {copy.selected(selected, pageCount)}
              </Badge>
            ) : null}
          </div>
        )}
      </Field>
    </div>
  );
}
