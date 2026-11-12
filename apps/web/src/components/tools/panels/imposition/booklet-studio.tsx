"use client";
// `pdf.booklet`: saddle-stitch imposition with a front/back signature preview.
import { Badge } from "@ariadshift/ui/components/badge";
import { useMemo, useState } from "react";
import { ChipGroup, controlClass, Field, useEmitParams } from "../organiser/panel-kit";
import type { CustomToolPanelProps } from "../types";
import { impositionCopy } from "./copy";
import { bookletSheets, PAPER_NAMES, type PaperName, sheetSize } from "./paper";

const copy = impositionCopy.booklet;
type Binding = "left" | "right";
const SAMPLE_PAGES = 8;
const MAX_SHEETS_SHOWN = 3;

function Spread({
  label,
  pair,
  pageCount,
  sheet,
}: {
  label: string;
  pair: readonly [number, number];
  pageCount: number;
  sheet: { w: number; h: number };
}) {
  const width = 140;
  const height = (width * sheet.h) / sheet.w;
  return (
    <figure className="grid justify-items-center gap-1" data-slot="booklet-spread">
      <svg
        role="img"
        aria-label={`${label}: ${pair.map((page) => (page > pageCount ? copy.blank : page)).join(", ")}`}
        viewBox="0 0 200 140"
        width={width}
        height={height}
        className="rounded-sm border border-border bg-background text-foreground"
      >
        <title>{label}</title>
        <line
          x1="100"
          y1="0"
          x2="100"
          y2="140"
          stroke="currentColor"
          strokeDasharray="4 3"
          strokeOpacity={0.5}
        />
        {pair.map((page, index) => (
          <text
            key={index}
            data-page={page > pageCount ? "blank" : page}
            x={index === 0 ? 50 : 150}
            y={70}
            fontSize={30}
            textAnchor="middle"
            dominantBaseline="central"
            fill="currentColor"
            fillOpacity={page > pageCount ? 0.4 : 1}
          >
            {page > pageCount ? copy.blank : page}
          </text>
        ))}
      </svg>
      <figcaption className="text-xs text-muted-foreground">{label}</figcaption>
    </figure>
  );
}

export function BookletStudio({
  tool,
  files,
  pageCount,
  disabled,
  onChange,
}: CustomToolPanelProps) {
  const [paper, setPaper] = useState<PaperName>("A4");
  const [binding, setBinding] = useState<Binding>("left");
  const [creepText, setCreepText] = useState("0");
  const creep = Number(creepText);
  const creepValid = creepText.trim() !== "" && Number.isFinite(creep) && creep >= 0;
  const params = useMemo(
    () => ({ sheet: paper, binding, creep_pt: creepValid ? creep : 0 }),
    [paper, binding, creepValid, creep],
  );
  const extraErrors = useMemo(
    () => (creepValid ? {} : { creep_pt: copy.creepInvalid }),
    [creepValid],
  );
  const validation = useEmitParams(tool, params, { extraErrors, onChange });

  const known = pageCount ?? files[0]?.pageCount ?? null;
  const pages = known ?? SAMPLE_PAGES;
  const sheets = useMemo(() => bookletSheets(pages, binding), [pages, binding]);
  const sheet = sheetSize(paper, true);

  return (
    <div className="grid gap-4">
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
        <span className="text-sm font-medium text-start">{copy.binding}</span>
        <ChipGroup<Binding>
          label={copy.binding}
          value={binding}
          disabled={disabled}
          onSelect={setBinding}
          options={[
            { value: "left", label: copy.left },
            { value: "right", label: copy.right },
          ]}
        />
      </div>
      <Field label={copy.creep} error={validation.errors.creep_pt}>
        {({ id, errorId, invalid }) => (
          <input
            id={id}
            type="number"
            min={0}
            step={0.1}
            disabled={disabled}
            aria-invalid={invalid}
            aria-describedby={invalid ? errorId : undefined}
            className={`${controlClass} w-28 text-start`}
            value={creepText}
            onChange={(event) => setCreepText(event.target.value)}
          />
        )}
      </Field>
      <section className="grid gap-3" data-slot="booklet-preview" aria-label={copy.preview}>
        <Badge variant="secondary" className="self-start" data-slot="booklet-sheet-count">
          {copy.sheets(sheets.length, pages)}
        </Badge>
        <div className="grid gap-3">
          {sheets.slice(0, MAX_SHEETS_SHOWN).map((entry, index) => (
            <div key={entry.front.join("-")} className="flex flex-wrap items-end gap-3">
              <span className="w-16 text-xs font-medium text-start">
                {copy.sheetLabel(index + 1)}
              </span>
              <Spread label={copy.front} pair={entry.front} pageCount={pages} sheet={sheet} />
              <Spread label={copy.back} pair={entry.back} pageCount={pages} sheet={sheet} />
            </div>
          ))}
          {sheets.length > MAX_SHEETS_SHOWN ? (
            <p className="text-xs text-muted-foreground text-start">
              {copy.more(sheets.length - MAX_SHEETS_SHOWN)}
            </p>
          ) : null}
        </div>
        {known === null ? (
          <p className="text-xs text-muted-foreground text-start">{copy.sample}</p>
        ) : null}
      </section>
    </div>
  );
}
