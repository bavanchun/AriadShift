"use client";

import { type PointerEvent, useRef } from "react";
import {
  type OverlayItem,
  type OverlaySettings,
  overlayItems,
  PAGE_HEIGHT,
  PAGE_WIDTH,
  qrBox,
  type WatermarkToolId,
} from "./watermark-model";

/** Body-text stand-ins so the overlay reads against something page-like. */
const BODY_LINES = Array.from({ length: 24 }, (_, index) => ({
  y: 130 + index * 26,
  width: 380 + ((index * 53) % 90),
}));

function Body() {
  return (
    <g fill="currentColor" opacity={0.12}>
      <rect x={60} y={64} width={220} height={14} rx={3} />
      {BODY_LINES.map((line) => (
        <rect key={line.y} x={60} y={line.y} width={line.width} height={7} rx={3} />
      ))}
    </g>
  );
}

function hash(text: string): number {
  let value = 2166136261;
  for (const char of text) value = Math.imul(value ^ char.charCodeAt(0), 16777619) >>> 0;
  return value;
}

const MODULES = 21;

/** A stand-in for the code: the three finder squares plus modules derived from the content. */
function qrCells(content: string): Array<{ x: number; y: number }> {
  const cells: Array<{ x: number; y: number }> = [];
  let state = hash(content) || 1;
  const finder = (x: number, y: number) => x < 8 && y < 8;
  for (let y = 0; y < MODULES; y += 1) {
    for (let x = 0; x < MODULES; x += 1) {
      state ^= state << 13;
      state >>>= 0;
      state ^= state >>> 17;
      state ^= state << 5;
      state >>>= 0;
      const inFinder = finder(x, y) || finder(MODULES - 1 - x, y) || finder(x, MODULES - 1 - y);
      if (!inFinder && state % 2 === 0) cells.push({ x, y });
    }
  }
  return cells;
}

function QrPlaceholder({ settings }: { settings: OverlaySettings }) {
  const box = qrBox(settings);
  const unit = box.size / (MODULES + 2 * 2);
  const origin = unit * 2;
  const finders = [
    { fx: 0, fy: 0 },
    { fx: MODULES - 7, fy: 0 },
    { fx: 0, fy: MODULES - 7 },
  ];
  return (
    <g
      data-testid="qr-placeholder"
      data-content={settings.content}
      transform={`translate(${box.x} ${box.y})`}
    >
      <title>QR code placeholder</title>
      {settings.qrBackground === "white" ? (
        <rect width={box.size} height={box.size} fill="#fff" stroke="#d4d4d8" strokeWidth={0.5} />
      ) : null}
      <g fill={settings.color}>
        {finders.map(({ fx, fy }) => (
          <g key={`${fx}-${fy}`}>
            <rect
              x={origin + fx * unit}
              y={origin + fy * unit}
              width={7 * unit}
              height={7 * unit}
            />
            <rect
              x={origin + (fx + 1) * unit}
              y={origin + (fy + 1) * unit}
              width={5 * unit}
              height={5 * unit}
              fill={settings.qrBackground === "white" ? "#fff" : "none"}
            />
            <rect
              x={origin + (fx + 2) * unit}
              y={origin + (fy + 2) * unit}
              width={3 * unit}
              height={3 * unit}
            />
          </g>
        ))}
        {qrCells(settings.content).map((cell) => (
          <rect
            key={`${cell.x}-${cell.y}`}
            x={origin + cell.x * unit}
            y={origin + cell.y * unit}
            width={unit}
            height={unit}
          />
        ))}
      </g>
    </g>
  );
}

function Mark({ item }: { item: OverlayItem }) {
  const width = item.text.length * item.size * 0.55;
  const left =
    item.textAnchor === "start"
      ? item.x
      : item.textAnchor === "middle"
        ? item.x - width / 2
        : item.x - width;
  return (
    <g transform={`rotate(${-item.rotation} ${item.x} ${item.y})`} opacity={item.opacity}>
      {item.border ? (
        <rect
          data-testid="overlay-border"
          x={left - item.size * 0.4}
          y={item.y - item.size * 1.05}
          width={width + item.size * 0.8}
          height={item.size * 1.5}
          rx={3}
          fill="none"
          stroke={item.color}
          strokeWidth={2}
        />
      ) : null}
      <text
        data-testid="overlay-text"
        x={item.x}
        y={item.y}
        textAnchor={item.textAnchor}
        fontSize={item.size}
        fontFamily="serif"
        fontWeight={700}
        fill={item.color}
      >
        {item.text}
      </text>
    </g>
  );
}

interface OverlayPreviewProps {
  tool: WatermarkToolId;
  settings: OverlaySettings;
  pageCount?: number;
  /** Called while the mark is dragged, with the new PDF-point offsets. */
  onOffset?: (offsetX: number, offsetY: number) => void;
  label?: string;
}

export function OverlayPreview({
  tool,
  settings,
  pageCount,
  onOffset,
  label,
}: OverlayPreviewProps) {
  const svg = useRef<SVGSVGElement>(null);
  const drag = useRef<{ x: number; y: number; offsetX: number; offsetY: number } | null>(null);
  const items = overlayItems(tool, settings, pageCount);
  const draggable = !!onOffset && tool !== "pdf.header_footer";

  const scale = () => {
    const rect = svg.current?.getBoundingClientRect();
    return rect && rect.width > 0 && rect.height > 0
      ? { x: PAGE_WIDTH / rect.width, y: PAGE_HEIGHT / rect.height }
      : null;
  };
  const down = (event: PointerEvent<SVGSVGElement>) => {
    if (!draggable) return;
    drag.current = {
      x: event.clientX,
      y: event.clientY,
      offsetX: settings.offsetX,
      offsetY: settings.offsetY,
    };
    try {
      event.currentTarget.setPointerCapture?.(event.pointerId);
    } catch {
      // Capture is a nicety; dragging still works while the pointer stays over the page.
    }
  };
  const move = (event: PointerEvent<SVGSVGElement>) => {
    const start = drag.current;
    const factor = scale();
    if (!start || !factor || !onOffset) return;
    onOffset(
      Math.round(start.offsetX + (event.clientX - start.x) * factor.x),
      Math.round(start.offsetY - (event.clientY - start.y) * factor.y),
    );
  };
  const up = () => {
    drag.current = null;
  };

  const overlay =
    tool === "pdf.qr" ? (
      <QrPlaceholder settings={settings} />
    ) : (
      items.map((item) => <Mark key={item.key} item={item} />)
    );
  const under = tool === "pdf.watermark" && settings.layer === "under";

  return (
    <svg
      ref={svg}
      data-slot="overlay-preview"
      viewBox={`0 0 ${PAGE_WIDTH} ${PAGE_HEIGHT}`}
      role="img"
      aria-label={label ?? "Page preview with the overlay"}
      style={{ direction: "ltr" }}
      onPointerDown={down}
      onPointerMove={move}
      onPointerUp={up}
      onPointerCancel={up}
      className={`aspect-[595/842] w-full max-w-72 touch-none rounded-md border border-input bg-white text-zinc-900 shadow-sm ${draggable ? "cursor-grab active:cursor-grabbing" : ""}`}
    >
      <rect width={PAGE_WIDTH} height={PAGE_HEIGHT} fill="#fff" />
      {under ? overlay : null}
      <Body />
      {under ? null : overlay}
    </svg>
  );
}
