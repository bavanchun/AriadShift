"use client";

import { Button } from "@ariadshift/ui/components/button";
import { type PointerEvent, useRef } from "react";
import { signCopy } from "./copy";

export const PAD_WIDTH = 400;
export const PAD_HEIGHT = 160;
const MAX_POINTS = 2000;
const MIN_STEP = 2;

export type PadPoint = readonly [number, number];

/**
 * Turns pad points into the engine's `vector_data`: space-separated `x,y` pairs in points,
 * relative to the lower-left corner of the signature box, with y pointing up.
 */
export function toVectorData(
  points: readonly PadPoint[],
  box: { width: number; height: number },
): string {
  return points
    .map(([x, y]) => {
      const px = (x / PAD_WIDTH) * box.width;
      const py = ((PAD_HEIGHT - y) / PAD_HEIGHT) * box.height;
      return `${Math.round(px * 10) / 10},${Math.round(py * 10) / 10}`;
    })
    .join(" ");
}

/** A pointer-driven drawing surface; one continuous line, exactly as it will be placed. */
export function SignaturePad({
  points,
  onChange,
  disabled,
}: {
  points: readonly PadPoint[];
  onChange: (points: PadPoint[]) => void;
  disabled?: boolean;
}) {
  const drawing = useRef(false);
  const copy = signCopy.draw;

  const locate = (event: PointerEvent<SVGSVGElement>): PadPoint => {
    const bounds = event.currentTarget.getBoundingClientRect();
    const x = bounds.width
      ? ((event.clientX - bounds.left) / bounds.width) * PAD_WIDTH
      : event.clientX;
    const y = bounds.height
      ? ((event.clientY - bounds.top) / bounds.height) * PAD_HEIGHT
      : event.clientY;
    return [
      Math.round(Math.min(PAD_WIDTH, Math.max(0, x)) * 10) / 10,
      Math.round(Math.min(PAD_HEIGHT, Math.max(0, y)) * 10) / 10,
    ];
  };
  const extend = (next: PadPoint) => {
    const last = points[points.length - 1];
    if (last && Math.hypot(next[0] - last[0], next[1] - last[1]) < MIN_STEP) return;
    if (points.length >= MAX_POINTS) return;
    onChange([...points, next]);
  };

  return (
    <div className="grid gap-2" data-slot="signature-pad">
      <svg
        role="img"
        aria-label={copy.pad}
        viewBox={`0 0 ${PAD_WIDTH} ${PAD_HEIGHT}`}
        className="h-40 w-full touch-none rounded-lg border border-dashed border-input bg-background"
        onPointerDown={(event) => {
          if (disabled) return;
          drawing.current = true;
          event.currentTarget.setPointerCapture?.(event.pointerId);
          extend(locate(event));
        }}
        onPointerMove={(event) => {
          if (drawing.current) extend(locate(event));
        }}
        onPointerUp={() => {
          drawing.current = false;
        }}
        onPointerCancel={() => {
          drawing.current = false;
        }}
      >
        <line
          x1="16"
          y1={PAD_HEIGHT - 32}
          x2={PAD_WIDTH - 16}
          y2={PAD_HEIGHT - 32}
          className="stroke-muted-foreground/40"
          strokeDasharray="4 4"
        />
        {points.length > 0 ? (
          <polyline
            data-slot="signature-stroke"
            points={points.map(([x, y]) => `${x},${y}`).join(" ")}
            fill="none"
            className="stroke-foreground"
            strokeWidth="2.5"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        ) : null}
      </svg>
      <div className="flex items-center justify-between gap-2">
        <p className="text-xs text-muted-foreground text-start">
          {points.length ? copy.points(points.length) : copy.hint}
        </p>
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={disabled || points.length === 0}
          onClick={() => onChange([])}
        >
          {copy.clear}
        </Button>
      </div>
    </div>
  );
}
