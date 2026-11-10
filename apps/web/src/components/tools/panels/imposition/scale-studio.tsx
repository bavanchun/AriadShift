"use client";
// `pdf.scale`: target paper, fit mode and the pages to scale, with an aspect-ratio preview.
import { Badge } from "@ariadshift/ui/components/badge";
import { useMemo, useState } from "react";
import { PageSelector } from "../organiser/page-selector";
import { ChipGroup, resolvePageCount, useEmitParams } from "../organiser/panel-kit";
import type { CustomToolPanelProps } from "../types";
import { impositionCopy } from "./copy";
import { PAPER_NAMES, PAPER_POINTS, type PaperName } from "./paper";

const copy = impositionCopy.scale;
type Fit = "contain" | "cover" | "stretch";
const SOURCE = PAPER_POINTS.A4;
const PREVIEW_WIDTH = 150;

/** Scale factors applied to the example page for each fit mode. */
export function scaleFactors(target: { w: number; h: number }, fit: Fit): { x: number; y: number } {
  const x = target.w / SOURCE.w;
  const y = target.h / SOURCE.h;
  if (fit === "stretch") return { x, y };
  const uniform = fit === "contain" ? Math.min(x, y) : Math.max(x, y);
  return { x: uniform, y: uniform };
}

export function ScaleStudio({ tool, files, pageCount, disabled, onChange }: CustomToolPanelProps) {
  const [to, setTo] = useState<PaperName>("A4");
  const [fit, setFit] = useState<Fit>("contain");
  const [pages, setPages] = useState("all");
  const count = resolvePageCount(pageCount, files);
  const params = useMemo(() => ({ to, fit, pages }), [to, fit, pages]);
  const validation = useEmitParams(tool, params, { pageCount: count, onChange });
  const target = PAPER_POINTS[to];
  const factors = scaleFactors(target, fit);
  const drawn = { w: SOURCE.w * factors.x, h: SOURCE.h * factors.y };
  const label = copy.preview(to, copy[fit]);
  return (
    <div className="grid gap-4 sm:grid-cols-[1fr_auto]">
      <div className="grid gap-4">
        <div className="grid gap-1.5">
          <span className="text-sm font-medium text-start">{impositionCopy.paper}</span>
          <ChipGroup<PaperName>
            label={impositionCopy.paper}
            value={to}
            disabled={disabled}
            onSelect={setTo}
            options={PAPER_NAMES.map((value) => ({ value, label: value }))}
          />
        </div>
        <div className="grid gap-1.5">
          <span className="text-sm font-medium text-start">{copy.fit}</span>
          <ChipGroup<Fit>
            label={copy.fit}
            value={fit}
            disabled={disabled}
            onSelect={setFit}
            options={[
              { value: "contain", label: copy.contain },
              { value: "cover", label: copy.cover },
              { value: "stretch", label: copy.stretch },
            ]}
          />
        </div>
        <PageSelector
          value={pages}
          onChange={setPages}
          pageCount={count}
          disabled={disabled}
          error={validation.errors.pages}
        />
      </div>
      <div className="grid content-start justify-items-center gap-2">
        <svg
          role="img"
          aria-label={label}
          data-slot="scale-preview"
          viewBox={`0 0 ${target.w} ${target.h}`}
          width={PREVIEW_WIDTH}
          height={(PREVIEW_WIDTH * target.h) / target.w}
          className="shrink-0 rounded-sm border border-border bg-muted/40 text-foreground"
        >
          <title>{label}</title>
          <rect
            x={(target.w - drawn.w) / 2}
            y={(target.h - drawn.h) / 2}
            width={drawn.w}
            height={drawn.h}
            fill="currentColor"
            fillOpacity={0.12}
            stroke="currentColor"
            strokeWidth={target.w / 100}
          />
        </svg>
        <Badge variant="secondary" data-slot="scale-readout">
          {copy.readout(Math.round(factors.x * 100), Math.round(factors.y * 100))}
        </Badge>
        <p className="text-xs text-muted-foreground">{copy.example}</p>
      </div>
    </div>
  );
}
