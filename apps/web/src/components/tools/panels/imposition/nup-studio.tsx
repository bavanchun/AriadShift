"use client";
// `pdf.nup`: pages per sheet, fill order, sheet size, margins, and a live sheet layout preview.
import { Badge } from "@ariadshift/ui/components/badge";
import { useMemo, useState } from "react";
import { ChipGroup, controlClass, Field, useEmitParams } from "../organiser/panel-kit";
import type { CustomToolPanelProps } from "../types";
import { impositionCopy } from "./copy";
import { gridDimensions, PAPER_NAMES, type PaperName, sheetSize } from "./paper";

const copy = impositionCopy.nup;
const PER_SHEET = [2, 4, 6, 8, 9, 16] as const;
type PerSheet = (typeof PER_SHEET)[number];
type Order = "row" | "column";
const PREVIEW_WIDTH = 180;

function SheetPreview({
  perSheet,
  order,
  borders,
  margin,
  sheet,
}: {
  perSheet: number;
  order: Order;
  borders: boolean;
  margin: number;
  sheet: { w: number; h: number };
}) {
  const { cols, rows } = gridDimensions(perSheet, sheet);
  const usableW = Math.max(sheet.w - 2 * margin, 1);
  const usableH = Math.max(sheet.h - 2 * margin, 1);
  const cellW = usableW / cols;
  const cellH = usableH / rows;
  const inset = Math.min(cellW, cellH) * 0.06;
  const fontSize = Math.min(cellW, cellH) * 0.35;
  const cells = Array.from({ length: perSheet }, (_, slot) => ({
    slot,
    col: order === "row" ? slot % cols : Math.floor(slot / rows),
    row: order === "row" ? Math.floor(slot / cols) : slot % rows,
  }));
  return (
    <svg
      role="img"
      aria-label={copy.preview(perSheet)}
      data-slot="nup-preview"
      data-cols={cols}
      data-rows={rows}
      viewBox={`0 0 ${sheet.w} ${sheet.h}`}
      width={PREVIEW_WIDTH}
      height={(PREVIEW_WIDTH * sheet.h) / sheet.w}
      className="shrink-0 rounded-sm border border-border bg-background text-foreground shadow-xs"
    >
      <title>{copy.preview(perSheet)}</title>
      {cells.map(({ slot, col, row }) => (
        <g key={slot} data-slot="nup-cell" data-page={slot + 1}>
          <rect
            x={margin + col * cellW + inset}
            y={margin + row * cellH + inset}
            width={Math.max(cellW - 2 * inset, 1)}
            height={Math.max(cellH - 2 * inset, 1)}
            fill="currentColor"
            fillOpacity={0.06}
            stroke="currentColor"
            strokeOpacity={borders ? 0.8 : 0.2}
            strokeWidth={borders ? sheet.w / 150 : sheet.w / 400}
          />
          <text
            x={margin + col * cellW + cellW / 2}
            y={margin + row * cellH + cellH / 2}
            fontSize={fontSize}
            textAnchor="middle"
            dominantBaseline="central"
            fill="currentColor"
          >
            {slot + 1}
          </text>
        </g>
      ))}
    </svg>
  );
}

export function NupStudio({ tool, files, pageCount, disabled, onChange }: CustomToolPanelProps) {
  const [perSheet, setPerSheet] = useState<PerSheet>(2);
  const [order, setOrder] = useState<Order>("row");
  const [paper, setPaper] = useState<PaperName>("A4");
  const [landscape, setLandscape] = useState(false);
  const [borders, setBorders] = useState(false);
  const [marginText, setMarginText] = useState("0");
  const margin = Number(marginText);
  const size = sheetSize(paper, landscape);
  const marginValid =
    marginText.trim() !== "" &&
    Number.isFinite(margin) &&
    margin >= 0 &&
    size.w - 2 * margin > 1 &&
    size.h - 2 * margin > 1;

  const params = useMemo<Record<string, unknown>>(
    () => ({
      per_sheet: perSheet,
      order,
      borders,
      margins_pt: marginValid ? margin : 0,
      sheet: landscape ? { w: size.w, h: size.h } : paper,
    }),
    [perSheet, order, borders, marginValid, margin, landscape, size.w, size.h, paper],
  );
  const extraErrors = useMemo(
    () => (marginValid ? {} : { margins_pt: copy.marginsInvalid }),
    [marginValid],
  );
  const validation = useEmitParams(tool, params, { extraErrors, onChange });
  const pages = pageCount ?? files[0]?.pageCount ?? null;

  return (
    <div className="grid gap-4 sm:grid-cols-[1fr_auto]">
      <div className="grid gap-4">
        <div className="grid gap-1.5">
          <span className="text-sm font-medium text-start">{copy.perSheet}</span>
          <ChipGroup<PerSheet>
            label={copy.perSheet}
            value={perSheet}
            disabled={disabled}
            onSelect={setPerSheet}
            options={PER_SHEET.map((value) => ({ value, label: String(value) }))}
          />
        </div>
        <div className="grid gap-1.5">
          <span className="text-sm font-medium text-start">{impositionCopy.paper}</span>
          <ChipGroup<PaperName>
            label={impositionCopy.paper}
            value={paper}
            disabled={disabled}
            onSelect={setPaper}
            options={PAPER_NAMES.map((value) => ({ value, label: value }))}
          />
        </div>
        <div className="grid gap-1.5">
          <span className="text-sm font-medium text-start">{copy.order}</span>
          <ChipGroup<Order>
            label={copy.order}
            value={order}
            disabled={disabled}
            onSelect={setOrder}
            options={[
              { value: "row", label: copy.row },
              { value: "column", label: copy.column },
            ]}
          />
        </div>
        <div className="flex flex-wrap gap-x-6 gap-y-2">
          <label className="flex items-center gap-2 text-sm text-start">
            <input
              type="checkbox"
              disabled={disabled}
              className="size-4 cursor-pointer rounded-sm border border-input accent-primary"
              checked={landscape}
              onChange={(event) => setLandscape(event.target.checked)}
            />
            {copy.landscape}
          </label>
          <label className="flex items-center gap-2 text-sm text-start">
            <input
              type="checkbox"
              disabled={disabled}
              className="size-4 cursor-pointer rounded-sm border border-input accent-primary"
              checked={borders}
              onChange={(event) => setBorders(event.target.checked)}
            />
            {copy.borders}
          </label>
        </div>
        <Field label={copy.margins} error={validation.errors.margins_pt}>
          {({ id, errorId, invalid }) => (
            <input
              id={id}
              type="number"
              min={0}
              step={1}
              disabled={disabled}
              aria-invalid={invalid}
              aria-describedby={invalid ? errorId : undefined}
              className={`${controlClass} w-28 text-start`}
              value={marginText}
              onChange={(event) => setMarginText(event.target.value)}
            />
          )}
        </Field>
      </div>
      <div className="grid content-start justify-items-center gap-2">
        <SheetPreview
          perSheet={perSheet}
          order={order}
          borders={borders}
          margin={marginValid ? margin : 0}
          sheet={size}
        />
        {pages !== null ? (
          <Badge variant="secondary" data-slot="nup-sheet-count">
            {copy.sheets(pages, Math.ceil(pages / perSheet))}
          </Badge>
        ) : null}
      </div>
    </div>
  );
}
