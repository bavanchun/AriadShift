"use client";

import {
  type KeyboardEvent,
  type ReactNode,
  type PointerEvent as ReactPointerEvent,
  useRef,
  useState,
} from "react";
import { useStore } from "zustand";
import {
  type Annotation,
  type AnnotationStore,
  boundsOf,
  draftFromGesture,
  type Point,
} from "./annotation-store";
import { annotationLabel, useAnnotateCopy } from "./copy";

export interface PageSize {
  /** Width and height of the page in PDF points. */
  width: number;
  height: number;
}

/** Distance an arrow key moves a selected annotation; shift moves it further. */
const NUDGE = 4;
const NUDGE_LARGE = 20;

type Live = { type: "drag"; from: Point; to: Point } | { type: "stroke"; points: Point[] } | null;

export interface AnnotationCanvasProps {
  store: AnnotationStore;
  page: number;
  size: PageSize;
  /** The rendered page underneath the drawing layer. */
  children?: ReactNode;
}

/**
 * The drawing layer of one page. The SVG uses the page's own point units, so annotations keep
 * their PDF coordinates at any zoom; `y` is flipped because PDF pages start at the bottom.
 */
export function AnnotationCanvas({ store, page, size, children }: AnnotationCanvasProps) {
  const copy = useAnnotateCopy();
  const state = useStore(store);
  const svgRef = useRef<SVGSVGElement>(null);
  const [live, setLive] = useState<Live>(null);
  const { tool } = state.settings;
  const drawing = tool !== "select";
  const flip = (y: number) => size.height - y;

  function toPage(event: { clientX: number; clientY: number }): Point {
    const box = svgRef.current?.getBoundingClientRect();
    const scaleX = box && box.width > 0 ? size.width / box.width : 1;
    const scaleY = box && box.height > 0 ? size.height / box.height : 1;
    return [
      (event.clientX - (box?.left ?? 0)) * scaleX,
      size.height - (event.clientY - (box?.top ?? 0)) * scaleY,
    ];
  }

  function onPointerDown(event: ReactPointerEvent<SVGSVGElement>) {
    if (!drawing || event.button !== 0) return;
    event.currentTarget.setPointerCapture?.(event.pointerId);
    const at = toPage(event);
    setLive(tool === "ink" ? { type: "stroke", points: [at] } : { type: "drag", from: at, to: at });
  }

  function onPointerMove(event: ReactPointerEvent<SVGSVGElement>) {
    if (!live) return;
    const at = toPage(event);
    setLive(
      live.type === "stroke"
        ? { type: "stroke", points: [...live.points, at] }
        : { ...live, to: at },
    );
  }

  function finish(event: ReactPointerEvent<SVGSVGElement>) {
    if (!live) return;
    event.currentTarget.releasePointerCapture?.(event.pointerId);
    const at = toPage(event);
    const gesture =
      live.type === "stroke"
        ? ({ type: "stroke", points: live.points } as const)
        : ({ type: "drag", from: live.from, to: at } as const);
    setLive(null);
    const draft = draftFromGesture({ ...state.settings, page }, gesture);
    if (draft) state.add(draft);
  }

  const onAnnotationKey = (id: string) => (event: KeyboardEvent<SVGGElement>) => {
    const step = event.shiftKey ? NUDGE_LARGE : NUDGE;
    const moves: Record<string, [number, number]> = {
      ArrowLeft: [-step, 0],
      ArrowRight: [step, 0],
      ArrowUp: [0, step],
      ArrowDown: [0, -step],
    };
    const move = moves[event.key];
    if (move) {
      event.preventDefault();
      state.move(id, move[0], move[1]);
    } else if (event.key === "Delete" || event.key === "Backspace") {
      event.preventDefault();
      state.remove(id);
    } else if (event.key === "Escape") {
      state.select(null);
    } else if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      state.select(id);
    }
  };

  const onPage = state.items.filter((item) => item.page === page);

  return (
    <div
      data-slot="annotation-canvas"
      className="relative w-full overflow-hidden rounded-lg border bg-card"
      style={{ aspectRatio: `${size.width} / ${size.height}` }}
    >
      {children}
      {/* biome-ignore lint/a11y/useKeyWithClickEvents: a background click only clears the selection; Escape does the same from an annotation */}
      {/* biome-ignore lint/a11y/useSemanticElements: an SVG drawing surface cannot be a fieldset */}
      <svg
        ref={svgRef}
        role="group"
        aria-label={copy.surface.canvas(page)}
        viewBox={`0 0 ${size.width} ${size.height}`}
        className={`absolute inset-0 size-full touch-none ${drawing ? "cursor-crosshair" : ""}`}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={finish}
        onPointerCancel={() => setLive(null)}
        onClick={(event) => {
          if (event.target === event.currentTarget) state.select(null);
        }}
      >
        {onPage.map((annotation) => (
          // biome-ignore lint/a11y/useSemanticElements: an SVG group cannot be a <button>
          <g
            key={annotation.id}
            role="button"
            tabIndex={0}
            data-annotation-id={annotation.id}
            data-kind={annotation.kind}
            data-selected={state.selectedId === annotation.id ? "true" : undefined}
            aria-pressed={state.selectedId === annotation.id}
            aria-label={annotationLabel(copy, annotation, state.items.indexOf(annotation))}
            aria-keyshortcuts="ArrowLeft ArrowRight ArrowUp ArrowDown Delete"
            className="cursor-pointer outline-none focus-visible:[&>rect[data-outline]]:stroke-[--color-ring]"
            onClick={(event) => {
              event.stopPropagation();
              if (!drawing) state.select(annotation.id);
            }}
            onKeyDown={onAnnotationKey(annotation.id)}
            style={{ pointerEvents: drawing ? "none" : "auto" }}
          >
            <AnnotationShape annotation={annotation} flip={flip} />
            {state.selectedId === annotation.id ? (
              <Outline annotation={annotation} flip={flip} />
            ) : null}
          </g>
        ))}
        {live ? <LivePreview live={live} flip={flip} color={state.settings.color} /> : null}
      </svg>
    </div>
  );
}

function Outline({ annotation, flip }: { annotation: Annotation; flip: (y: number) => number }) {
  const [left, bottom, right, top] = boundsOf(annotation);
  return (
    <rect
      data-outline=""
      x={left - 2}
      y={flip(top) - 2}
      width={right - left + 4}
      height={top - bottom + 4}
      fill="none"
      stroke="currentColor"
      strokeDasharray="4 3"
      strokeWidth={1}
      className="text-primary"
    />
  );
}

function AnnotationShape({
  annotation,
  flip,
}: {
  annotation: Annotation;
  flip: (y: number) => number;
}) {
  switch (annotation.kind) {
    case "highlight": {
      const [left, bottom, right, top] = annotation.rect;
      return (
        <rect
          x={left}
          y={flip(top)}
          width={right - left}
          height={top - bottom}
          fill={annotation.color}
          fillOpacity={annotation.opacity}
        />
      );
    }
    case "underline":
    case "strike": {
      const [left, bottom, right, top] = annotation.rect;
      const y = flip(annotation.kind === "underline" ? bottom : (bottom + top) / 2);
      return (
        <>
          <rect
            x={left}
            y={flip(top)}
            width={right - left}
            height={top - bottom}
            fill="transparent"
          />
          <line
            x1={left}
            x2={right}
            y1={y}
            y2={y}
            stroke={annotation.color}
            strokeOpacity={annotation.opacity}
            strokeWidth={1.5}
          />
        </>
      );
    }
    case "note": {
      const [left, bottom, right, top] = annotation.rect;
      return (
        <g data-icon={annotation.icon}>
          <rect
            x={left}
            y={flip(top)}
            width={right - left}
            height={top - bottom}
            rx={3}
            fill={annotation.color}
            fillOpacity={annotation.opacity}
            stroke="#00000066"
          />
          <title>{annotation.contents}</title>
        </g>
      );
    }
    case "ink":
      return (
        <>
          {annotation.strokes.map((stroke, index) => (
            <polyline
              key={index}
              points={stroke.map((p) => `${p[0]},${flip(p[1])}`).join(" ")}
              fill="none"
              stroke={annotation.color}
              strokeOpacity={annotation.opacity}
              strokeWidth={annotation.lineWidth}
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          ))}
        </>
      );
    case "shape": {
      const stroke = {
        stroke: annotation.color,
        strokeOpacity: annotation.opacity,
        strokeWidth: annotation.lineWidth,
      };
      const [left, bottom, right, top] = annotation.rect;
      if (annotation.shape === "rectangle" || annotation.shape === "ellipse") {
        const fill = annotation.fill ?? "none";
        return annotation.shape === "rectangle" ? (
          <rect
            x={left}
            y={flip(top)}
            width={right - left}
            height={top - bottom}
            fill={fill}
            {...stroke}
          />
        ) : (
          <ellipse
            cx={(left + right) / 2}
            cy={flip((bottom + top) / 2)}
            rx={(right - left) / 2}
            ry={(top - bottom) / 2}
            fill={fill}
            {...stroke}
          />
        );
      }
      const [x1, y1] = annotation.from;
      const [x2, y2] = annotation.to;
      return (
        <>
          <line x1={x1} y1={flip(y1)} x2={x2} y2={flip(y2)} {...stroke} />
          {annotation.shape === "arrow" ? (
            <polyline
              points={arrowHead(x1, flip(y1), x2, flip(y2), 8 + annotation.lineWidth * 2)}
              fill="none"
              {...stroke}
            />
          ) : null}
        </>
      );
    }
    case "stamp": {
      const [left, bottom, right, top] = annotation.rect;
      return (
        <g>
          <rect
            x={left}
            y={flip(top)}
            width={right - left}
            height={top - bottom}
            rx={4}
            fill="none"
            stroke={annotation.color}
            strokeOpacity={annotation.opacity}
            strokeWidth={2}
          />
          <text
            x={(left + right) / 2}
            y={flip((bottom + top) / 2)}
            textAnchor="middle"
            dominantBaseline="central"
            fill={annotation.color}
            fillOpacity={annotation.opacity}
            fontWeight={700}
            fontSize={Math.max(8, Math.min((top - bottom) * 0.5, 18))}
          >
            {annotation.label}
          </text>
        </g>
      );
    }
  }
}

/** The two barbs of an arrow head at `(x2, y2)`, in view coordinates. */
function arrowHead(x1: number, y1: number, x2: number, y2: number, length: number): string {
  const angle = Math.atan2(y2 - y1, x2 - x1);
  const barb = (offset: number) =>
    `${x2 - length * Math.cos(angle + offset)},${y2 - length * Math.sin(angle + offset)}`;
  return `${barb(Math.PI / 7)} ${x2},${y2} ${barb(-Math.PI / 7)}`;
}

function LivePreview({
  live,
  flip,
  color,
}: {
  live: NonNullable<Live>;
  flip: (y: number) => number;
  color: string;
}) {
  if (live.type === "stroke") {
    return (
      <polyline
        points={live.points.map((p) => `${p[0]},${flip(p[1])}`).join(" ")}
        fill="none"
        stroke={color}
        strokeWidth={2}
        strokeLinecap="round"
        pointerEvents="none"
      />
    );
  }
  const x = Math.min(live.from[0], live.to[0]);
  const top = Math.max(live.from[1], live.to[1]);
  return (
    <rect
      x={x}
      y={flip(top)}
      width={Math.abs(live.to[0] - live.from[0])}
      height={Math.abs(live.to[1] - live.from[1])}
      fill="none"
      stroke={color}
      strokeDasharray="4 3"
      pointerEvents="none"
    />
  );
}
