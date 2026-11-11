"use client";
// Extract, delete and rotate: a page selection, plus the rotation picker for rotate.
import { useMemo, useState } from "react";
import type { CustomToolPanelProps } from "../types";
import { organiserCopy } from "./copy";
import { expandPageRange, hasDuplicates } from "./page-range";
import { PageSelector } from "./page-selector";
import { ChipGroup, useEmitParams } from "./panel-kit";

type Props = CustomToolPanelProps & { pageCount?: number };

/** `pdf.extract_pages` and `pdf.delete_pages`: one required page selection. */
export function PageSelectionStudio({ tool, pageCount, disabled, onChange }: Props) {
  const deleting = tool.id === "pdf.delete_pages";
  const [pages, setPages] = useState("1");
  const params = useMemo(() => ({ pages }), [pages]);
  const extraErrors = useMemo(() => {
    const expanded = pageCount === undefined ? null : expandPageRange(pages, pageCount);
    if (!expanded) return {};
    if (deleting && hasDuplicates(expanded)) return { pages: organiserCopy.selector.repeats };
    if (deleting && new Set(expanded).size >= (pageCount ?? 0))
      return { pages: organiserCopy.selector.keepOne };
    return {};
  }, [pages, pageCount, deleting]);
  const validation = useEmitParams(tool, params, { pageCount, extraErrors, onChange });
  return (
    <PageSelector
      value={pages}
      onChange={setPages}
      pageCount={pageCount}
      disabled={disabled}
      error={validation.errors.pages}
    />
  );
}

const ANGLES = [
  { value: -90, label: organiserCopy.rotate.ccw },
  { value: 90, label: organiserCopy.rotate.cw },
  { value: 180, label: organiserCopy.rotate.half },
] as const;

/** A miniature page with a corner mark, turned by the chosen angle. */
function RotatedPageIcon({ angle, size = 28 }: { angle: number; size?: number }) {
  return (
    <span
      data-slot="rotation-icon"
      data-rotation={angle}
      aria-hidden="true"
      className="inline-block rounded-xs border border-foreground/40 bg-background"
      style={{ width: size * 0.72, height: size, transform: `rotate(${angle}deg)` }}
    >
      <span className="m-0.5 block size-1.5 rounded-full bg-primary" />
    </span>
  );
}

/** `pdf.rotate`: angle chips with a rotated page preview, and the pages to turn. */
export function RotateStudio({ tool, pageCount, disabled, onChange }: Props) {
  const [angle, setAngle] = useState<number>(90);
  const [pages, setPages] = useState("all");
  const params = useMemo(() => ({ angle, pages }), [angle, pages]);
  const extraErrors = useMemo(() => {
    const expanded = pageCount === undefined ? null : expandPageRange(pages, pageCount);
    return expanded && hasDuplicates(expanded) ? { pages: organiserCopy.selector.repeats } : {};
  }, [pages, pageCount]);
  const validation = useEmitParams(tool, params, { pageCount, extraErrors, onChange });
  return (
    <div className="grid gap-4">
      <div className="grid gap-2">
        <span className="text-sm font-medium text-start">{organiserCopy.rotate.angle}</span>
        <div className="flex flex-wrap items-center gap-3">
          <ChipGroup<number>
            label={organiserCopy.rotate.angle}
            value={angle}
            disabled={disabled}
            onSelect={setAngle}
            options={ANGLES.map((entry) => ({
              value: entry.value,
              label: (
                <>
                  <RotatedPageIcon angle={entry.value} size={14} />
                  {entry.label}
                </>
              ),
              name: entry.label,
            }))}
          />
          <div
            data-slot="rotation-preview"
            role="img"
            aria-label={organiserCopy.rotate.preview(angle)}
            className="flex size-14 items-center justify-center rounded-lg border border-border bg-muted/40"
          >
            <RotatedPageIcon angle={angle} size={36} />
          </div>
        </div>
      </div>
      <PageSelector
        value={pages}
        onChange={setPages}
        pageCount={pageCount}
        disabled={disabled}
        error={validation.errors.pages}
      />
    </div>
  );
}
