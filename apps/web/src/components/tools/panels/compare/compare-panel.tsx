"use client";

import { Button } from "@ariadshift/ui/components/button";
import { ArrowLeftRightIcon } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { validatePageRange } from "@/components/tools/params-form";
import {
  ChoiceChips,
  type CustomToolPanelProps,
  controlClass,
  ErrorText,
  Field,
  sectionTitleClass,
  useEmitParams,
} from "../ocr/panel-kit";

export const COMPARE_TOOLS = ["pdf.compare"] as const;
export type CompareToolId = (typeof COMPARE_TOOLS)[number];

export function isCompareTool(id: string): id is CompareToolId {
  return (COMPARE_TOOLS as readonly string[]).includes(id);
}

export type CompareView = "side-by-side" | "overlay" | "text";

/** How each view maps onto the tool's `mode` parameter. */
export const VIEW_MODES: Record<CompareView, "both" | "visual" | "text"> = {
  "side-by-side": "both",
  overlay: "visual",
  text: "text",
};

export interface CompareViewOptions {
  /** Colour that marks changed regions in the overlay. */
  highlight: string;
  /** Share of a region's pixels that must differ before it counts as a change, 0-100. */
  threshold: number;
}

export interface ComparePanelProps extends CustomToolPanelProps {
  /** Reports the files in run order, baseline first, whenever the choice or the swap changes. */
  onInputsChange?: (ids: string[]) => void;
  /** Reports how the diff viewer should draw the overlay; these are display settings only. */
  onViewOptionsChange?: (options: CompareViewOptions) => void;
}

const copy = {
  baseline: "Baseline (File A)",
  revised: "Revised (File B)",
  swap: "A ⇄ B",
  swapLabel: "Swap baseline and revised",
  pages: "Page range",
  pagesHint: "Leave empty to compare every page. Examples: 1-5, odd, 2,4",
  view: "Comparison view",
  views: [
    { value: "text", label: "Text Diff List" },
    { value: "side-by-side", label: "Side-by-Side (Desktop)" },
    { value: "overlay", label: "Visual Overlay (Desktop)" },
  ] as const,
  viewHint: {
    "side-by-side":
      "Shows both documents next to each other with the text and visual differences marked. Page rendering needs the desktop engine, so run it with the ashift command.",
    overlay:
      "Lays the revised page over the baseline and highlights the pixels that changed. Page rendering needs the desktop engine, so run it with the ashift command.",
    text: "Lists every inserted and removed passage of text, page by page.",
  },
  highlight: "Highlight colour",
  threshold: "Sensitivity threshold",
  thresholdHint: "Display setting for the diff viewer. A higher threshold ignores smaller changes.",
  needTwo: "Add a second PDF to compare. Comparison needs exactly two files.",
  same: "Choose two different files.",
  sample: {
    title: "Preview",
    removed: "Net 30 days",
    added: "Net 45 days",
    page: "Page 2",
  },
};

const tooShort = (files: unknown[]) => files.length < 2;

function FileSelect({
  label,
  value,
  files,
  onChange,
  disabled,
}: {
  label: string;
  value: string;
  files: CustomToolPanelProps["files"];
  onChange: (id: string) => void;
  disabled?: boolean;
}) {
  return (
    <Field label={label}>
      {({ id, invalid: _invalid, ...aria }) => (
        <select
          {...aria}
          id={id}
          className={`${controlClass} w-full`}
          disabled={disabled}
          value={value}
          onChange={(event) => onChange(event.target.value)}
        >
          {files.map((file) => (
            <option key={file.id} value={file.id}>
              {file.name}
            </option>
          ))}
        </select>
      )}
    </Field>
  );
}

/** A diagram of what the chosen view produces; it is not a render of the two documents. */
function ViewPreview({ view, highlight }: { view: CompareView; highlight: string }) {
  const line = (key: string, width: string) => (
    <div key={key} className="h-2 rounded-sm bg-stone-400/70" style={{ width }} />
  );
  return (
    <section
      aria-label={copy.sample.title}
      data-slot="compare-preview"
      data-view={view}
      className="grid gap-2 rounded-xl border border-input bg-card p-3"
    >
      {view === "side-by-side" ? (
        <div className="grid grid-cols-2 gap-3">
          {(["A", "B"] as const).map((side) => (
            <div
              key={side}
              className="grid gap-1.5 rounded-md border border-input bg-background p-2"
            >
              <span className="text-xs text-muted-foreground">{side}</span>
              {line("1", "80%")}
              <div
                className="h-2 rounded-sm"
                style={{
                  width: side === "A" ? "60%" : "75%",
                  backgroundColor: highlight,
                  opacity: 0.55,
                }}
              />
              {line("3", "70%")}
            </div>
          ))}
        </div>
      ) : view === "overlay" ? (
        <div className="relative grid gap-1.5 rounded-md border border-input bg-background p-2">
          {line("1", "80%")}
          <div
            className="h-2 rounded-sm"
            style={{ width: "75%", backgroundColor: highlight, opacity: 0.55 }}
          />
          {line("3", "70%")}
        </div>
      ) : (
        <ul className="grid gap-1 font-mono text-xs">
          <li className="text-muted-foreground">{copy.sample.page}</li>
          <li className="rounded-sm bg-destructive/10 px-1.5 text-destructive">
            − {copy.sample.removed}
          </li>
          <li className="rounded-sm bg-primary/10 px-1.5 text-primary">+ {copy.sample.added}</li>
        </ul>
      )}
      <p className="text-xs text-muted-foreground text-start">{copy.viewHint[view]}</p>
    </section>
  );
}

/** Baseline and revised document selection plus the comparison view for `pdf.compare`. */
export function ComparePanel({
  tool,
  files,
  disabled,
  onChange,
  onInputsChange,
  onViewOptionsChange,
}: ComparePanelProps) {
  const [baselineId, setBaselineId] = useState(files[0]?.id ?? "");
  const [revisedId, setRevisedId] = useState(files[1]?.id ?? "");
  const [pages, setPages] = useState("");
  const [view, setView] = useState<CompareView>("text");
  const [highlight, setHighlight] = useState("#dc2626");
  const [threshold, setThreshold] = useState(10);

  // Files are re-ingested under new ids; an id that vanished falls back to the file's position.
  const baseline = files.find((file) => file.id === baselineId) ?? files[0];
  const revised = files.find((file) => file.id === revisedId) ?? files[1];
  const orderedIds = baseline && revised ? [baseline.id, revised.id] : [];
  const orderKey = orderedIds.join("\n");

  const inputsRef = useRef(onInputsChange);
  const optionsRef = useRef(onViewOptionsChange);
  useEffect(() => {
    inputsRef.current = onInputsChange;
    optionsRef.current = onViewOptionsChange;
  }, [onInputsChange, onViewOptionsChange]);
  useEffect(() => {
    if (orderKey) inputsRef.current?.(orderKey.split("\n"));
  }, [orderKey]);
  useEffect(() => {
    optionsRef.current?.({ highlight, threshold });
  }, [highlight, threshold]);

  const knownCounts = [baseline?.pageCount, revised?.pageCount].filter(
    (count): count is number => typeof count === "number",
  );
  const comparablePages = knownCounts.length ? Math.min(...knownCounts) : undefined;
  const pagesError = pages.trim() ? validatePageRange(pages.trim(), comparablePages) : null;

  const params = useMemo<Record<string, unknown>>(
    () => ({ mode: VIEW_MODES[view], ...(pages.trim() ? { pages: pages.trim() } : {}) }),
    [view, pages],
  );
  useEmitParams(tool, params, {
    extraErrors: {
      ...(tooShort(files) ? { inputs: copy.needTwo } : {}),
      ...(baseline && revised && baseline.id === revised.id ? { inputs: copy.same } : {}),
      ...(pagesError ? { pages: pagesError } : {}),
    },
    onChange,
  });

  const swap = () => {
    if (!baseline || !revised) return;
    setBaselineId(revised.id);
    setRevisedId(baseline.id);
  };
  const showOverlayControls = view !== "text";

  return (
    <div data-slot="compare-panel" data-tool={tool.id} className="grid gap-4 text-start">
      <fieldset className="grid gap-3 border-0 p-0" disabled={disabled}>
        <legend className={sectionTitleClass}>
          {copy.baseline} / {copy.revised}
        </legend>
        <div className="grid items-end gap-3 sm:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)]">
          <FileSelect
            label={copy.baseline}
            value={baseline?.id ?? ""}
            files={files}
            onChange={setBaselineId}
            disabled={disabled}
          />
          <Button
            type="button"
            variant="outline"
            aria-label={copy.swapLabel}
            disabled={disabled || tooShort(files)}
            onClick={swap}
            className="mb-0.5"
          >
            <ArrowLeftRightIcon aria-hidden="true" />
            {copy.swap}
          </Button>
          <FileSelect
            label={copy.revised}
            value={revised?.id ?? ""}
            files={files}
            onChange={setRevisedId}
            disabled={disabled}
          />
        </div>
        <ErrorText>
          {tooShort(files)
            ? copy.needTwo
            : baseline && revised && baseline.id === revised.id
              ? copy.same
              : undefined}
        </ErrorText>
      </fieldset>

      <Field label={copy.pages} hint={copy.pagesHint} error={pagesError ?? undefined}>
        {({ id, invalid: _invalid, ...aria }) => (
          <input
            {...aria}
            id={id}
            type="text"
            autoComplete="off"
            spellCheck={false}
            placeholder="all"
            className={`${controlClass} w-full`}
            disabled={disabled}
            value={pages}
            onChange={(event) => setPages(event.target.value)}
          />
        )}
      </Field>

      <div className="grid gap-1.5">
        <p className={sectionTitleClass}>{copy.view}</p>
        <ChoiceChips
          label={copy.view}
          options={copy.views}
          value={view}
          onSelect={setView}
          disabled={disabled}
        />
      </div>

      {showOverlayControls ? (
        <fieldset
          className="grid gap-3 border-0 p-0"
          disabled={disabled}
          data-slot="compare-overlay-controls"
        >
          <Field label={copy.highlight}>
            {({ id, invalid: _invalid, ...aria }) => (
              <input
                {...aria}
                id={id}
                type="color"
                className="h-9 w-16 cursor-pointer rounded-lg border border-input bg-background p-1"
                value={highlight}
                onChange={(event) => setHighlight(event.target.value)}
              />
            )}
          </Field>
          <Field label={`${copy.threshold}: ${threshold}%`} hint={copy.thresholdHint}>
            {({ id, invalid: _invalid, ...aria }) => (
              <input
                {...aria}
                id={id}
                type="range"
                min={0}
                max={100}
                step={1}
                className="w-full accent-primary"
                value={threshold}
                onChange={(event) => setThreshold(Number(event.target.value))}
              />
            )}
          </Field>
        </fieldset>
      ) : null}

      <ViewPreview view={view} highlight={highlight} />
    </div>
  );
}
