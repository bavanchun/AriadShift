"use client";
// `pdf.insert_blank`: where to insert, how many blanks, and the blank page size.
import { Badge } from "@ariadshift/ui/components/badge";
import { useMemo, useState } from "react";
import type { CustomToolPanelProps } from "../types";
import { organiserCopy } from "./copy";
import { expandPageRange } from "./page-range";
import { PageSelector } from "./page-selector";
import { ChipGroup, controlClass, Field, PagePill, useEmitParams } from "./panel-kit";

const copy = organiserCopy.blank;
const SIZES = ["same", "A4", "Letter", "Legal"] as const;
type Size = (typeof SIZES)[number];
const PREVIEW_PAGES = 8;

export function InsertBlankStudio({
  tool,
  pageCount,
  disabled,
  onChange,
}: CustomToolPanelProps & { pageCount?: number }) {
  const [after, setAfter] = useState("all");
  const [count, setCount] = useState("1");
  const [size, setSize] = useState<Size>("same");
  const countNumber = Number(count);
  const countValid = Number.isInteger(countNumber) && countNumber >= 1;
  const params = useMemo<Record<string, unknown>>(
    () => ({ after, ...(countValid ? { count: countNumber } : {}), size }),
    [after, countValid, countNumber, size],
  );
  const extraErrors = useMemo(() => (countValid ? {} : { count: copy.countInvalid }), [countValid]);
  const validation = useEmitParams(tool, params, { pageCount, extraErrors, onChange });

  const selected = useMemo(() => {
    const pages = pageCount === undefined ? null : expandPageRange(after, pageCount);
    return pages ? new Set(pages) : null;
  }, [after, pageCount]);
  const strip = useMemo(() => {
    if (!pageCount || !selected || !countValid) return [];
    const items: Array<{ key: string; page: number | null }> = [];
    for (let page = 1; page <= Math.min(pageCount, PREVIEW_PAGES); page++) {
      items.push({ key: `p${page}`, page });
      if (selected.has(page))
        for (let blank = 0; blank < Math.min(countNumber, 3); blank++)
          items.push({ key: `b${page}-${blank}`, page: null });
    }
    return items;
  }, [pageCount, selected, countValid, countNumber]);
  const total =
    pageCount !== undefined && selected && countValid
      ? pageCount + selected.size * countNumber
      : null;

  return (
    <div className="grid gap-4">
      <PageSelector
        label={copy.after}
        value={after}
        onChange={setAfter}
        pageCount={pageCount}
        disabled={disabled}
        error={validation.errors.after}
      />
      <div className="grid gap-4 sm:grid-cols-2">
        <Field label={copy.count} error={validation.errors.count}>
          {({ id, errorId, invalid }) => (
            <input
              id={id}
              type="number"
              min={1}
              step={1}
              disabled={disabled}
              aria-invalid={invalid}
              aria-describedby={invalid ? errorId : undefined}
              className={`${controlClass} w-28 text-start`}
              value={count}
              onChange={(event) => setCount(event.target.value)}
            />
          )}
        </Field>
        <div className="grid gap-1.5">
          <span className="text-sm font-medium text-start">{copy.size}</span>
          <ChipGroup<Size>
            label={copy.size}
            value={size}
            disabled={disabled}
            onSelect={setSize}
            options={SIZES.map((value) => ({ value, label: value === "same" ? copy.same : value }))}
          />
        </div>
      </div>
      {strip.length > 0 ? (
        <div className="grid gap-2" data-slot="blank-preview">
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-sm font-medium text-start">{copy.strip}</span>
            {total !== null && pageCount !== undefined ? (
              <Badge variant="secondary" data-slot="blank-total">
                {copy.result(pageCount, total)}
              </Badge>
            ) : null}
          </div>
          <div className="flex flex-wrap gap-1">
            {strip.map((item) =>
              item.page === null ? (
                <PagePill key={item.key} tone="blank" label={copy.blank} />
              ) : (
                <PagePill key={item.key} label={item.page} />
              ),
            )}
            {pageCount !== undefined && pageCount > PREVIEW_PAGES ? (
              <span aria-hidden="true">…</span>
            ) : null}
          </div>
        </div>
      ) : null}
    </div>
  );
}
