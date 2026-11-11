"use client";

import { useId } from "react";
import type { TextStylePatch } from "./page-edit-session";
import { DEFAULT_PAGE_MODE_LABELS, type PageModeLabels } from "./page-mode-labels";
import type { PageBBox, PageObject } from "./page-model";

export const FONT_FAMILY_CHOICES = [
  "Liberation Sans",
  "Liberation Serif",
  "Liberation Mono",
  "Noto Sans",
] as const;

export interface PageObjectInspectorProps {
  object?: PageObject;
  /** The live bounding box while the object is being dragged; defaults to the object's own. */
  liveBBox?: PageBBox;
  onStyleChange?: (patch: TextStylePatch) => void;
  labels?: Partial<PageModeLabels>;
  className?: string;
}

const pt = (value: number) => Math.round(value * 10) / 10;

/** Live coordinates plus, for a text run, font family, size and colour. */
export function PageObjectInspector({
  object,
  liveBBox,
  onStyleChange,
  labels,
  className,
}: PageObjectInspectorProps) {
  const copy = { ...DEFAULT_PAGE_MODE_LABELS, ...labels };
  const ids = useId();
  const fieldClass =
    "h-8 w-full rounded-lg border border-input bg-background px-2 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50";

  if (!object) {
    return (
      <section data-slot="page-object-inspector" aria-label={copy.inspector} className={className}>
        <p className="text-sm text-muted-foreground">{copy.noSelection}</p>
      </section>
    );
  }

  const [left, bottom, right, top] = liveBBox ?? object.bbox;
  const readout: [string, number][] = [
    [copy.x, left],
    [copy.y, bottom],
    [copy.w, right - left],
    [copy.h, top - bottom],
  ];
  const families: string[] = [...FONT_FAMILY_CHOICES];

  return (
    <section
      data-slot="page-object-inspector"
      aria-label={copy.inspector}
      className={`flex flex-col gap-3 ${className ?? ""}`}
    >
      <dl aria-label={copy.position} className="grid grid-cols-4 gap-2 text-xs">
        {readout.map(([name, value]) => (
          <div key={name} className="flex flex-col">
            <dt className="text-muted-foreground">{name}</dt>
            <dd className="m-0 font-mono tabular-nums" data-testid={`readout-${name}`}>
              {pt(value)}
            </dd>
          </div>
        ))}
      </dl>
      {object.kind === "text" && (
        <div className="flex flex-col gap-2">
          <label className="flex flex-col gap-1 text-xs font-medium" htmlFor={`${ids}-family`}>
            {copy.fontFamily}
            <select
              id={`${ids}-family`}
              className={fieldClass}
              value={families.includes(object.fontName) ? object.fontName : ""}
              onChange={(event) => onStyleChange?.({ fontName: event.target.value })}
            >
              {!families.includes(object.fontName) && (
                <option value="">{copy.fontFamilyOriginal}</option>
              )}
              {families.map((family) => (
                <option key={family} value={family}>
                  {family}
                </option>
              ))}
            </select>
          </label>
          <label className="flex flex-col gap-1 text-xs font-medium" htmlFor={`${ids}-size`}>
            {copy.fontSize}
            <input
              id={`${ids}-size`}
              type="number"
              min={1}
              max={1000}
              step={0.5}
              className={fieldClass}
              value={object.fontSize}
              onChange={(event) => onStyleChange?.({ fontSize: event.target.valueAsNumber })}
            />
          </label>
          <label className="flex flex-col gap-1 text-xs font-medium" htmlFor={`${ids}-color`}>
            {copy.color}
            <input
              id={`${ids}-color`}
              type="color"
              className="h-8 w-16 rounded-lg border border-input bg-background p-1"
              value={object.color.slice(0, 7)}
              onChange={(event) => onStyleChange?.({ color: event.target.value })}
            />
          </label>
        </div>
      )}
    </section>
  );
}
