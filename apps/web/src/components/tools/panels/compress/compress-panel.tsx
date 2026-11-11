"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { cn } from "@ariadshift/ui/lib/utils";
import { useId, useMemo, useState } from "react";
import {
  buildCompressParams,
  COMPRESS_PRESETS,
  type CompressPresetId,
  type CompressSettings,
  compressionRatio,
  DEFAULT_SETTINGS,
  DPI_MAX,
  DPI_MIN,
  estimateSize,
  QUALITY_MAX,
  QUALITY_MIN,
} from "./compress-model";
import { type CustomToolPanelProps, formatBytes, useEmitParams, VALID } from "./panel-contract";

const GRAYSCALE_RATIO = 0.75;
const LINEARIZE_RATIO = 1;

export function CompressPanel({ tool, files, disabled, locale, onChange }: CustomToolPanelProps) {
  const prefix = useId();
  const [settings, setSettings] = useState<CompressSettings>(DEFAULT_SETTINGS);
  const isCompress = tool.id === "pdf.compress";
  const isGrayscale = tool.id === "pdf.grayscale";

  const params = useMemo(
    () => (isCompress ? buildCompressParams(settings) : {}),
    [isCompress, settings],
  );
  useEmitParams(params, VALID, onChange);

  const originalBytes = files[0]?.size;
  const ratio = isCompress
    ? compressionRatio(settings)
    : isGrayscale
      ? GRAYSCALE_RATIO
      : LINEARIZE_RATIO;
  const estimate = originalBytes === undefined ? null : estimateSize(originalBytes, ratio);

  const choosePreset = (id: CompressPresetId) => {
    const preset = COMPRESS_PRESETS.find((item) => item.id === id);
    if (!preset) return;
    setSettings((previous) => ({
      ...previous,
      preset: id,
      dpi: preset.dpi ?? null,
      quality: preset.quality ?? null,
    }));
  };
  // Moving a slider leaves the named profile: the explicit values are the request now.
  const setDpi = (dpi: number) => setSettings((previous) => ({ ...previous, preset: null, dpi }));
  const setQuality = (quality: number) =>
    setSettings((previous) => ({ ...previous, preset: null, quality }));

  return (
    <div data-slot="compress-panel" className="grid gap-5">
      {isCompress ? (
        <>
          <fieldset className="grid gap-2" disabled={disabled}>
            <legend className="mb-1 text-sm font-medium">Compression level</legend>
            <div className="grid gap-2 sm:grid-cols-2">
              {COMPRESS_PRESETS.map((preset) => {
                const selected = settings.preset === preset.id;
                return (
                  <button
                    key={preset.id}
                    type="button"
                    aria-pressed={selected}
                    data-preset={preset.id}
                    disabled={disabled}
                    onClick={() => choosePreset(preset.id)}
                    className={cn(
                      "grid cursor-pointer gap-0.5 rounded-lg border px-3 py-2.5 text-start outline-none transition-colors focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50",
                      selected
                        ? "border-primary bg-primary/5"
                        : "border-input bg-background hover:bg-muted",
                    )}
                  >
                    <span className="text-sm font-medium">
                      {preset.label} · {preset.detail}
                    </span>
                  </button>
                );
              })}
            </div>
          </fieldset>

          <div className="grid gap-1.5">
            <label htmlFor={`${prefix}-dpi`} className="flex justify-between text-sm font-medium">
              <span>Image resolution</span>
              <output htmlFor={`${prefix}-dpi`} data-testid="dpi-value">
                {settings.dpi === null ? "Original" : `${settings.dpi} DPI`}
              </output>
            </label>
            <input
              id={`${prefix}-dpi`}
              type="range"
              min={DPI_MIN}
              max={DPI_MAX}
              step={1}
              disabled={disabled}
              value={settings.dpi ?? DPI_MAX}
              onChange={(event) => setDpi(Number(event.target.value))}
              className="w-full accent-primary"
            />
          </div>

          <div className="grid gap-1.5">
            <label
              htmlFor={`${prefix}-quality`}
              className="flex justify-between text-sm font-medium"
            >
              <span>Image quality</span>
              <output htmlFor={`${prefix}-quality`} data-testid="quality-value">
                {settings.quality === null ? "Original" : `${settings.quality}%`}
              </output>
            </label>
            <input
              id={`${prefix}-quality`}
              type="range"
              min={QUALITY_MIN}
              max={QUALITY_MAX}
              step={1}
              disabled={disabled}
              value={settings.quality ?? QUALITY_MAX}
              onChange={(event) => setQuality(Number(event.target.value))}
              className="w-full accent-primary"
            />
          </div>

          <label className="flex cursor-pointer items-center gap-2 text-sm">
            <input
              type="checkbox"
              className="size-4 cursor-pointer rounded-sm border border-input accent-primary"
              disabled={disabled}
              checked={settings.grayscale}
              onChange={(event) =>
                setSettings((previous) => ({ ...previous, grayscale: event.target.checked }))
              }
            />
            Convert images to grayscale
          </label>
        </>
      ) : null}

      {isGrayscale ? <GrayscalePreview /> : null}
      {tool.id === "pdf.linearize" ? (
        <p className="text-sm text-muted-foreground">
          Reorders the file for fast web view, so the first page shows before the whole file has
          downloaded. Content and quality are unchanged and the size stays about the same.
        </p>
      ) : null}

      <section
        aria-label="Estimated size"
        data-slot="size-estimate"
        className="grid gap-1.5 rounded-lg border border-input bg-muted/40 p-3"
      >
        <h3 className="text-sm font-medium">Estimated size</h3>
        {estimate && originalBytes !== undefined ? (
          <p className="flex flex-wrap items-center gap-2 text-sm" data-testid="size-estimate">
            <span>{formatBytes(originalBytes, locale)}</span>
            <span aria-hidden="true">→</span>
            <span>
              ~{formatBytes(estimate.low, locale)}–{formatBytes(estimate.high, locale)}
            </span>
            <Badge variant={estimate.reductionPercent > 0 ? "secondary" : "outline"}>
              {estimate.reductionPercent > 0 ? `-${estimate.reductionPercent}%` : "≈ 0%"}
            </Badge>
          </p>
        ) : (
          <p className="text-sm text-muted-foreground">Add a file to see the estimated size.</p>
        )}
        <p className="text-xs text-muted-foreground">
          An estimate from the chosen settings; the real size depends on how many images the file
          holds.
        </p>
      </section>
    </div>
  );
}

function GrayscalePreview() {
  return (
    <section aria-label="Grayscale preview" data-slot="grayscale-preview" className="grid gap-2">
      <div className="grid grid-cols-2 gap-3" dir="ltr">
        {(["Before", "After"] as const).map((label) => (
          <figure key={label} className="grid gap-1.5">
            <div
              role="img"
              aria-label={`${label}: ${label === "Before" ? "colour" : "monochrome"} page`}
              data-variant={label.toLowerCase()}
              className={cn(
                "aspect-[3/4] rounded-md border border-input bg-linear-to-br from-rose-500 via-amber-400 to-sky-500",
                label === "After" && "grayscale",
              )}
            />
            <figcaption className="text-center text-xs text-muted-foreground">{label}</figcaption>
          </figure>
        ))}
      </div>
      <p className="text-sm text-muted-foreground">
        Colours become shades of grey. Text stays sharp and its contrast is preserved; light colours
        such as yellow can become hard to read in print.
      </p>
    </section>
  );
}
