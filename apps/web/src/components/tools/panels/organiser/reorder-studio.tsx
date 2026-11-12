"use client";
// `pdf.reorder`: a named preset or an explicit page sequence, with the resulting order shown.
import { useMemo, useState } from "react";
import type { CustomToolPanelProps } from "../types";
import { organiserCopy } from "./copy";
import { expandPageRange } from "./page-range";
import { PageSelector } from "./page-selector";
import { ChipGroup, PagePill, useEmitParams } from "./panel-kit";

const copy = organiserCopy.reorder;
type Preset = "reverse" | "duplex-interleave" | "odd-then-even" | "custom";
const PREVIEW_LIMIT = 24;

/** The page order a named preset produces for a document of `pageCount` pages. */
export function presetOrder(preset: Exclude<Preset, "custom">, pageCount: number): number[] {
  const pages = Array.from({ length: pageCount }, (_, index) => index + 1);
  if (preset === "reverse") return pages.reverse();
  if (preset === "odd-then-even")
    return [...pages.filter((page) => page % 2 === 1), ...pages.filter((page) => page % 2 === 0)];
  return pages.map((_, index) => (index % 2 === 0 ? 1 + index / 2 : pageCount - (index - 1) / 2));
}

export function ReorderStudio({
  tool,
  pageCount,
  disabled,
  onChange,
}: CustomToolPanelProps & { pageCount?: number }) {
  const [preset, setPreset] = useState<Preset>("reverse");
  const [custom, setCustom] = useState("");
  const params = useMemo(
    () => ({ order: preset === "custom" ? custom : preset }),
    [preset, custom],
  );
  const validation = useEmitParams(tool, params, { pageCount, onChange });
  const order = useMemo(() => {
    if (pageCount === undefined) return null;
    if (preset !== "custom") return presetOrder(preset, pageCount);
    return expandPageRange(custom, pageCount);
  }, [preset, custom, pageCount]);
  return (
    <div className="grid gap-4">
      <ChipGroup<Preset>
        label={copy.preset}
        value={preset}
        disabled={disabled}
        onSelect={setPreset}
        options={[
          { value: "reverse", label: copy.reverse },
          { value: "duplex-interleave", label: copy.duplex },
          { value: "odd-then-even", label: copy.oddEven },
          { value: "custom", label: copy.custom },
        ]}
      />
      {preset === "custom" ? (
        <PageSelector
          chips={false}
          label={copy.customInput}
          hint={copy.customHint}
          value={custom}
          onChange={setCustom}
          pageCount={pageCount}
          disabled={disabled}
          error={validation.errors.order}
        />
      ) : null}
      {order && order.length > 0 ? (
        <div className="grid gap-2" data-slot="reorder-preview">
          <span className="text-sm font-medium text-start">{copy.result}</span>
          <ol className="flex flex-wrap gap-1" aria-label={copy.result}>
            {order.slice(0, PREVIEW_LIMIT).map((page, index) => (
              <li key={`${index}-${page}`}>
                <PagePill label={page} />
              </li>
            ))}
            {order.length > PREVIEW_LIMIT ? <li aria-hidden="true">…</li> : null}
          </ol>
        </div>
      ) : null}
    </div>
  );
}
