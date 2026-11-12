"use client";

import { cn } from "@ariadshift/ui/lib/utils";
import { useId } from "react";
import { CONTROL_CLASS } from "../compress/panel-contract";
import { OverlayPreview } from "./overlay-preview";
import { defaultSettings, type OverlaySettings } from "./watermark-model";

interface RemovalStudioProps {
  settings: OverlaySettings;
  onChange: (patch: Partial<OverlaySettings>) => void;
  error?: string;
  pageCount?: number;
  disabled?: boolean;
}

/**
 * Removal works on identifiers the detector issues, never on a pattern, so the engine cannot
 * strip page content by accident. The before/after pair shows what one candidate looks like.
 */
export function RemovalStudio({
  settings,
  onChange,
  error,
  pageCount,
  disabled,
}: RemovalStudioProps) {
  const prefix = useId();
  const before: OverlaySettings = {
    ...defaultSettings("pdf.watermark"),
    text: settings.previewText,
  };
  const after: OverlaySettings = { ...before, text: "" };
  return (
    <div data-slot="watermark-removal" className="grid gap-5">
      <ol className="grid gap-1.5 ps-5 text-sm text-muted-foreground [list-style:decimal]">
        <li>
          Run <strong className="font-medium text-foreground">Find watermarks</strong> first. It
          lists text and images that repeat on most pages as candidates, each with an identifier.
        </li>
        <li>Paste the identifiers of the candidates to remove, separated by commas or spaces.</li>
        <li>Only the candidates you list are removed; wildcards are never accepted.</li>
      </ol>

      <div className="grid gap-1.5">
        <label htmlFor={`${prefix}-candidates`} className="text-sm font-medium">
          Candidate identifiers
        </label>
        <textarea
          id={`${prefix}-candidates`}
          className={cn(CONTROL_CLASS, "min-h-20 w-full py-2 font-mono text-xs")}
          spellCheck={false}
          disabled={disabled}
          aria-invalid={!!error}
          aria-describedby={error ? `${prefix}-candidates-error` : undefined}
          value={settings.candidates}
          onChange={(event) => onChange({ candidates: event.target.value })}
        />
        {error ? (
          <p id={`${prefix}-candidates-error`} role="alert" className="text-sm text-destructive">
            {error}
          </p>
        ) : null}
      </div>

      <div className="grid gap-1.5">
        <label htmlFor={`${prefix}-preview-text`} className="text-sm font-medium">
          Watermark text in the preview
        </label>
        <input
          id={`${prefix}-preview-text`}
          className={cn(CONTROL_CLASS, "h-9 w-full")}
          disabled={disabled}
          value={settings.previewText}
          onChange={(event) => onChange({ previewText: event.target.value })}
        />
        <p className="text-xs text-muted-foreground">
          Only changes the sample below, to help you recognise the candidate.
        </p>
      </div>

      <div className="grid grid-cols-2 gap-3" dir="ltr">
        <figure className="grid justify-items-center gap-1.5">
          <OverlayPreview
            tool="pdf.watermark"
            settings={before}
            pageCount={pageCount}
            label="Before: page with the watermark"
          />
          <figcaption className="text-xs text-muted-foreground">Before</figcaption>
        </figure>
        <figure className="grid justify-items-center gap-1.5">
          <OverlayPreview
            tool="pdf.watermark"
            settings={after}
            pageCount={pageCount}
            label="After: page without the watermark"
          />
          <figcaption className="text-xs text-muted-foreground">After</figcaption>
        </figure>
      </div>
    </div>
  );
}
