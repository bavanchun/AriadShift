"use client";
// `pdf.single_page`: joins every page into one continuous page. The tool takes no parameters.
import { Badge } from "@ariadshift/ui/components/badge";
import { useMemo } from "react";
import { resolvePageCount, useEmitParams } from "../organiser/panel-kit";
import type { CustomToolPanelProps } from "../types";
import { impositionCopy } from "./copy";

const copy = impositionCopy.single;
const STRIP_PAGES = 5;
const NO_PARAMS = {};

export function SinglePageStudio({ tool, files, pageCount, onChange }: CustomToolPanelProps) {
  const count = resolvePageCount(pageCount, files) ?? null;
  const params = useMemo(() => NO_PARAMS, []);
  useEmitParams(tool, params, { onChange });
  const shown = Math.min(count ?? STRIP_PAGES, STRIP_PAGES);
  return (
    <div className="grid gap-3" data-slot="single-page-studio">
      <p className="text-sm text-muted-foreground text-start">{copy.noOptions}</p>
      <div
        role="img"
        aria-label={copy.preview}
        data-slot="single-page-strip"
        className="flex max-h-56 w-24 flex-col overflow-hidden rounded-sm border border-border bg-background"
      >
        {Array.from({ length: shown }, (_, index) => index + 1).map((page) => (
          <div
            key={page}
            data-slot="strip-page"
            className="flex h-12 shrink-0 items-center justify-center border-b border-dashed border-border text-xs text-muted-foreground last:border-b-0"
          >
            {page}
          </div>
        ))}
        {count !== null && count > shown ? (
          <div className="py-1 text-center text-xs text-muted-foreground">…</div>
        ) : null}
      </div>
      <Badge variant="secondary" className="self-start" data-slot="single-page-summary">
        {copy.summary(count)}
      </Badge>
    </div>
  );
}
