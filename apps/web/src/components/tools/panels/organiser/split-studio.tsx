"use client";
// `pdf.split`: page ranges, every N pages, or visual split points between page pills.
import { Badge } from "@ariadshift/ui/components/badge";
import { Fragment, useMemo, useState } from "react";
import type { CustomToolPanelProps } from "../types";
import { organiserCopy } from "./copy";
import { ChipGroup, controlClass, ErrorText, Field, PagePill, useEmitParams } from "./panel-kit";

const copy = organiserCopy.split;
type Mode = "ranges" | "every" | "points";
/** Beyond this many pages the pill strip stops being usable, so ranges take over. */
export const MAX_SPLIT_POINT_PAGES = 200;

/** Turns split-after markers into inclusive page ranges (`1-3`, `4`, `5-12`). */
export function rangesFromSplitPoints(pageCount: number, points: ReadonlySet<number>): string[] {
  const ranges: string[] = [];
  let start = 1;
  for (let page = 1; page <= pageCount; page++) {
    if (page === pageCount || points.has(page)) {
      ranges.push(start === page ? String(page) : `${start}-${page}`);
      start = page + 1;
    }
  }
  return ranges;
}

export function SplitStudio({
  tool,
  pageCount,
  disabled,
  onChange,
}: CustomToolPanelProps & { pageCount?: number }) {
  const [mode, setMode] = useState<Mode>("ranges");
  const [rangesText, setRangesText] = useState(() => {
    if (!pageCount || pageCount < 2) return "1-last";
    const half = Math.ceil(pageCount / 2);
    return `1-${half}, ${half + 1}-last`;
  });
  const [every, setEvery] = useState("1");
  const [points, setPoints] = useState<ReadonlySet<number>>(new Set());

  const entries = useMemo(
    () =>
      rangesText
        .split(",")
        .map((part) => part.trim())
        .filter(Boolean),
    [rangesText],
  );
  const everyNumber = Number(every);
  const everyValid = Number.isInteger(everyNumber) && everyNumber >= 1;
  const pointsUsable = pageCount !== undefined && pageCount <= MAX_SPLIT_POINT_PAGES;

  const params = useMemo<Record<string, unknown>>(() => {
    if (mode === "ranges") return { mode, ranges: entries };
    if (mode === "every") return everyValid ? { mode, every: everyNumber } : { mode };
    return { mode: "ranges", ranges: pageCount ? rangesFromSplitPoints(pageCount, points) : [] };
  }, [mode, entries, everyValid, everyNumber, points, pageCount]);

  const extraErrors = useMemo((): Record<string, string> => {
    if (mode === "ranges" && entries.length === 0)
      return { ranges: "Enter at least one page range." };
    if (mode === "every" && !everyValid) return { every: copy.everyInvalid };
    if (mode === "points" && pageCount === undefined) return { ranges: copy.needsPages };
    if (mode === "points" && !pointsUsable) return { ranges: copy.pointsTooMany };
    return {};
  }, [mode, entries.length, everyValid, pageCount, pointsUsable]);

  const validation = useEmitParams(tool, params, { pageCount, extraErrors, onChange });

  const outputs =
    mode === "ranges"
      ? entries.length
      : mode === "every"
        ? everyValid && pageCount
          ? Math.ceil(pageCount / everyNumber)
          : null
        : points.size + 1;

  const toggle = (page: number) =>
    setPoints((previous) => {
      const next = new Set(previous);
      if (next.has(page)) next.delete(page);
      else next.add(page);
      return next;
    });

  return (
    <div className="grid gap-4">
      <ChipGroup<Mode>
        label={copy.mode}
        value={mode}
        disabled={disabled}
        onSelect={setMode}
        options={[
          { value: "ranges", label: copy.ranges },
          { value: "every", label: copy.every },
          { value: "points", label: copy.points },
        ]}
      />

      {mode === "ranges" ? (
        <Field label={copy.rangesInput} hint={copy.rangesHint} error={validation.errors.ranges}>
          {({ id, errorId, invalid }) => (
            <input
              id={id}
              type="text"
              spellCheck={false}
              autoComplete="off"
              disabled={disabled}
              aria-invalid={invalid}
              aria-describedby={invalid ? errorId : undefined}
              className={`${controlClass} w-full text-start`}
              value={rangesText}
              onChange={(event) => setRangesText(event.target.value)}
            />
          )}
        </Field>
      ) : null}

      {mode === "every" ? (
        <Field label={copy.everyInput} error={validation.errors.every}>
          {({ id, errorId, invalid }) => (
            <input
              id={id}
              type="number"
              min={1}
              step={1}
              disabled={disabled}
              aria-invalid={invalid}
              aria-describedby={invalid ? errorId : undefined}
              className={`${controlClass} w-32 text-start`}
              value={every}
              onChange={(event) => setEvery(event.target.value)}
            />
          )}
        </Field>
      ) : null}

      {mode === "points" ? (
        <div className="grid gap-2">
          <p className="text-xs text-muted-foreground text-start">{copy.pointsHint}</p>
          {pointsUsable && pageCount ? (
            // biome-ignore lint/a11y/useSemanticElements: a strip of pages with toggle buttons between them
            <div
              role="group"
              aria-label={copy.pillList}
              className="flex flex-wrap items-center gap-y-2"
            >
              {Array.from({ length: pageCount }, (_, index) => index + 1).map((page) => (
                <Fragment key={page}>
                  <PagePill label={page} />
                  {page < pageCount ? (
                    <button
                      type="button"
                      disabled={disabled}
                      aria-pressed={points.has(page)}
                      aria-label={copy.marker(page)}
                      onClick={() => toggle(page)}
                      className="mx-0.5 flex h-9 w-3 items-center justify-center rounded-sm outline-none focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50 [&>span]:h-7 [&>span]:w-0.5 [&>span]:rounded-full [&>span]:bg-border hover:[&>span]:bg-primary/60 aria-pressed:[&>span]:w-1 aria-pressed:[&>span]:bg-primary"
                    >
                      <span />
                    </button>
                  ) : null}
                </Fragment>
              ))}
            </div>
          ) : (
            <ErrorText>{validation.errors.ranges}</ErrorText>
          )}
        </div>
      ) : null}

      {outputs !== null ? (
        <Badge variant="secondary" data-slot="split-output-count" className="self-start">
          {copy.outputs(outputs)}
        </Badge>
      ) : null}
    </div>
  );
}
