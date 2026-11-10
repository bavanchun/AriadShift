"use client";
// `pdf.crop`: draggable crop edges on a page, numeric margins, trim presets and a size readout.
import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { type KeyboardEvent, type PointerEvent, useMemo, useRef, useState } from "react";
import { PageSelector } from "../organiser/page-selector";
import {
  ChipGroup,
  controlClass,
  ErrorText,
  resolvePageCount,
  useEmitParams,
} from "../organiser/panel-kit";
import type { CustomToolPanelProps } from "../types";
import { impositionCopy } from "./copy";
import { PAPER_NAMES, PAPER_POINTS, type PaperName, toMillimetres } from "./paper";

const copy = impositionCopy.crop;
type Side = "top" | "right" | "bottom" | "left";
type Margins = Record<Side, number>;
const SIDES: readonly Side[] = ["top", "right", "bottom", "left"];
const MIN_BOX = 1;
const PREVIEW_WIDTH = 220;
const PRESETS = [
  { key: "none", points: 0 },
  { key: "quarter", points: 18 },
  { key: "half", points: 36 },
  { key: "inch", points: 72 },
] as const;

const round1 = (value: number) => Math.round(value * 10) / 10;
const round2 = (value: number) => Math.round(value * 100) / 100;

/** Keeps one margin inside the page while leaving at least `MIN_BOX` points of box. */
export function clampMargin(
  side: Side,
  value: number,
  margins: Margins,
  page: { w: number; h: number },
) {
  const opposite = { top: "bottom", bottom: "top", left: "right", right: "left" }[side] as Side;
  const span = side === "top" || side === "bottom" ? page.h : page.w;
  return Math.min(Math.max(value, 0), span - margins[opposite] - MIN_BOX);
}

export function CropStudio({ tool, files, pageCount, disabled, onChange }: CustomToolPanelProps) {
  const [paper, setPaper] = useState<PaperName>("A4");
  const [margins, setMargins] = useState<Margins>({ top: 0, right: 0, bottom: 0, left: 0 });
  const [pages, setPages] = useState("all");
  const [target, setTarget] = useState<"crop" | "media">("crop");
  const [dragging, setDragging] = useState<Side | null>(null);
  const svg = useRef<SVGSVGElement>(null);
  const count = resolvePageCount(pageCount, files);
  const page = PAPER_POINTS[paper];

  const width = page.w - margins.left - margins.right;
  const height = page.h - margins.top - margins.bottom;
  const negative = SIDES.some((side) => !Number.isFinite(margins[side]) || margins[side] < 0);
  const boxError = negative
    ? copy.negative
    : width < MIN_BOX || height < MIN_BOX
      ? copy.empty
      : null;

  const params = useMemo(
    () => ({
      box: {
        left: round2(margins.left),
        bottom: round2(margins.bottom),
        right: round2(page.w - margins.right),
        top: round2(page.h - margins.top),
      },
      pages,
      target,
    }),
    [margins, page.w, page.h, pages, target],
  );
  const extraErrors = useMemo(() => (boxError ? { box: boxError } : {}), [boxError]);
  const validation = useEmitParams(tool, params, { pageCount: count, extraErrors, onChange });

  const setMargin = (side: Side, value: number) =>
    setMargins((previous) => ({ ...previous, [side]: clampMargin(side, value, previous, page) }));
  const move = (event: PointerEvent<SVGSVGElement>) => {
    if (!dragging || !svg.current) return;
    const rect = svg.current.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return;
    const x = ((event.clientX - rect.left) / rect.width) * page.w;
    const y = ((event.clientY - rect.top) / rect.height) * page.h;
    const edge = { left: x, right: page.w - x, top: y, bottom: page.h - y }[dragging];
    setMargin(dragging, Math.round(edge));
  };
  const key = (side: Side) => (event: KeyboardEvent) => {
    // Arrow keys move the edge; the margin grows when the edge moves toward the page centre.
    const grow = { left: "ArrowRight", right: "ArrowLeft", top: "ArrowDown", bottom: "ArrowUp" }[
      side
    ];
    const shrink = { left: "ArrowLeft", right: "ArrowRight", top: "ArrowUp", bottom: "ArrowDown" }[
      side
    ];
    const step = event.shiftKey ? 10 : 1;
    if (event.key === grow) setMargin(side, margins[side] + step);
    else if (event.key === shrink) setMargin(side, margins[side] - step);
    else return;
    event.preventDefault();
  };

  const edges: Record<Side, { x: number; y: number; w: number; h: number; cursor: string }> = {
    left: {
      x: margins.left - 6,
      y: margins.top,
      w: 12,
      h: Math.max(height, 0),
      cursor: "ew-resize",
    },
    right: {
      x: page.w - margins.right - 6,
      y: margins.top,
      w: 12,
      h: Math.max(height, 0),
      cursor: "ew-resize",
    },
    top: { x: margins.left, y: margins.top - 6, w: Math.max(width, 0), h: 12, cursor: "ns-resize" },
    bottom: {
      x: margins.left,
      y: page.h - margins.bottom - 6,
      w: Math.max(width, 0),
      h: 12,
      cursor: "ns-resize",
    },
  };

  return (
    <div className="grid gap-4 sm:grid-cols-[1fr_auto]">
      <div className="grid gap-4">
        <div className="grid gap-1.5">
          <span className="text-sm font-medium text-start">{copy.reference}</span>
          <ChipGroup<PaperName>
            label={copy.reference}
            value={paper}
            disabled={disabled}
            onSelect={(next) => {
              setPaper(next);
              setMargins({ top: 0, right: 0, bottom: 0, left: 0 });
            }}
            options={PAPER_NAMES.map((value) => ({ value, label: value }))}
          />
        </div>
        <div className="grid gap-1.5">
          <span className="text-sm font-medium text-start">{copy.presets}</span>
          {/* biome-ignore lint/a11y/useSemanticElements: a labelled group of buttons */}
          <div role="group" aria-label={copy.presets} className="flex flex-wrap gap-1.5">
            {PRESETS.map((preset) => (
              <Button
                key={preset.key}
                type="button"
                size="sm"
                variant="outline"
                disabled={disabled}
                onClick={() =>
                  setMargins({
                    top: preset.points,
                    right: preset.points,
                    bottom: preset.points,
                    left: preset.points,
                  })
                }
              >
                {copy[preset.key]}
              </Button>
            ))}
          </div>
        </div>
        <fieldset className="grid grid-cols-2 gap-3" disabled={disabled}>
          <legend className="mb-1.5 text-sm font-medium text-start">{copy.margins}</legend>
          {SIDES.map((side) => (
            <label key={side} className="grid gap-1 text-xs text-muted-foreground text-start">
              {copy.margin(side)}
              <input
                type="number"
                min={0}
                step={1}
                className={`${controlClass} text-foreground text-start`}
                aria-invalid={!!boxError}
                value={Number.isFinite(margins[side]) ? margins[side] : ""}
                onChange={(event) =>
                  setMargins((previous) => ({
                    ...previous,
                    [side]: event.target.value === "" ? 0 : Number(event.target.value),
                  }))
                }
              />
            </label>
          ))}
        </fieldset>
        <ErrorText>{validation.errors.box}</ErrorText>
        <div className="grid gap-1.5">
          <span className="text-sm font-medium text-start">{copy.target}</span>
          <ChipGroup<"crop" | "media">
            label={copy.target}
            value={target}
            disabled={disabled}
            onSelect={setTarget}
            options={[
              { value: "crop", label: copy.crop },
              { value: "media", label: copy.media },
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
        {/* biome-ignore lint/a11y/useSemanticElements: an SVG canvas that holds the draggable edge sliders */}
        <svg
          ref={svg}
          role="group"
          aria-label={copy.preview}
          data-slot="crop-preview"
          viewBox={`0 0 ${page.w} ${page.h}`}
          width={PREVIEW_WIDTH}
          height={(PREVIEW_WIDTH * page.h) / page.w}
          className="shrink-0 touch-none select-none rounded-sm border border-border bg-background text-foreground shadow-xs"
          onPointerMove={move}
          onPointerUp={() => setDragging(null)}
          onPointerCancel={() => setDragging(null)}
          onPointerLeave={() => setDragging(null)}
        >
          <title>{copy.preview}</title>
          <path
            data-slot="crop-shade"
            fillRule="evenodd"
            fill="currentColor"
            fillOpacity={0.35}
            d={`M0 0H${page.w}V${page.h}H0Z M${margins.left} ${margins.top}H${page.w - margins.right}V${page.h - margins.bottom}H${margins.left}Z`}
          />
          <rect
            data-slot="crop-box"
            x={margins.left}
            y={margins.top}
            width={Math.max(width, 0)}
            height={Math.max(height, 0)}
            fill="none"
            stroke="var(--color-primary, currentColor)"
            strokeWidth={page.w / 150}
            strokeDasharray={`${page.w / 40} ${page.w / 80}`}
          />
          {SIDES.map((side) => (
            <rect
              key={side}
              role="slider"
              tabIndex={disabled ? -1 : 0}
              aria-label={copy.margin(side)}
              aria-orientation={side === "left" || side === "right" ? "horizontal" : "vertical"}
              aria-valuemin={0}
              aria-valuemax={Math.round(side === "top" || side === "bottom" ? page.h : page.w)}
              aria-valuenow={Math.round(margins[side])}
              data-side={side}
              x={edges[side].x}
              y={edges[side].y}
              width={edges[side].w}
              height={edges[side].h}
              fill="var(--color-primary, currentColor)"
              fillOpacity={dragging === side ? 0.5 : 0.15}
              style={{ cursor: edges[side].cursor }}
              onPointerDown={(event) => {
                if (disabled) return;
                event.preventDefault();
                setDragging(side);
              }}
              onKeyDown={disabled ? undefined : key(side)}
            />
          ))}
        </svg>
        <Badge variant="secondary" data-slot="crop-readout">
          {copy.readout(
            round1(Math.max(width, 0)),
            round1(Math.max(height, 0)),
            round1(toMillimetres(Math.max(width, 0))),
            round1(toMillimetres(Math.max(height, 0))),
          )}
        </Badge>
        <p className="max-w-[220px] text-xs text-muted-foreground text-start">{copy.note}</p>
      </div>
    </div>
  );
}
