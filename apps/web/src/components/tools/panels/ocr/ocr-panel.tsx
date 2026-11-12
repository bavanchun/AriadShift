"use client";

import { useMemo, useState } from "react";
import type { ToolDescriptor } from "@/converter/tool-catalog";
import { ocrCopy } from "./copy";
import { DEFAULT_OCR_LANGUAGES, filterLanguages, languageName } from "./languages";
import {
  CheckRow,
  Chip,
  ChoiceChips,
  type CustomToolPanelProps,
  controlClass,
  Field,
  numberDraft,
  sectionTitleClass,
  useEmitParams,
} from "./panel-kit";
import { TextLayerPreview } from "./text-layer-preview";

export const OCR_TOOLS = ["pdf.ocr", "image.ocr", "pdf.deskew", "pdf.auto_rotate"] as const;
export type OcrToolId = (typeof OCR_TOOLS)[number];

export function isOcrTool(id: string): id is OcrToolId {
  return (OCR_TOOLS as readonly string[]).includes(id);
}

type Mode = "skip-text" | "force" | "redo";

/** The `minimum` and `maximum` the registry declares for a numeric parameter. */
function bounds(tool: ToolDescriptor, key: string, fallback: [number, number]): [number, number] {
  const properties = tool.params_schema.properties as
    | Record<string, { minimum?: number; maximum?: number }>
    | undefined;
  const property = properties?.[key];
  return [property?.minimum ?? fallback[0], property?.maximum ?? fallback[1]];
}

function NumberField({
  label,
  hint,
  draft,
  onDraft,
  error,
  range,
  step,
  disabled,
}: {
  label: string;
  hint?: string;
  draft: string;
  onDraft: (draft: string) => void;
  error?: string;
  range: [number, number];
  step?: number | "any";
  disabled?: boolean;
}) {
  return (
    <Field label={label} hint={hint} error={error}>
      {({ id, invalid: _invalid, ...aria }) => (
        <input
          {...aria}
          id={id}
          type="number"
          inputMode="decimal"
          className={`${controlClass} w-full`}
          min={range[0]}
          max={range[1]}
          step={step ?? "any"}
          disabled={disabled}
          value={draft}
          onChange={(event) => onDraft(event.target.value)}
        />
      )}
    </Field>
  );
}

function PagesField({
  value,
  onChange,
  error,
  disabled,
}: {
  value: string;
  onChange: (value: string) => void;
  error?: string;
  disabled?: boolean;
}) {
  return (
    <Field label={ocrCopy.fields.pages} hint={ocrCopy.fields.pagesHint} error={error}>
      {({ id, invalid: _invalid, ...aria }) => (
        <input
          {...aria}
          id={id}
          type="text"
          autoComplete="off"
          spellCheck={false}
          className={`${controlClass} w-full`}
          disabled={disabled}
          value={value}
          onChange={(event) => onChange(event.target.value)}
        />
      )}
    </Field>
  );
}

function LanguagePicker({
  selected,
  onChange,
  disabled,
}: {
  selected: string[];
  onChange: (languages: string[]) => void;
  disabled?: boolean;
}) {
  const copy = ocrCopy.languages;
  const [query, setQuery] = useState("");
  const visible = useMemo(() => filterLanguages(query), [query]);
  const toggle = (code: string) =>
    onChange(
      selected.includes(code) ? selected.filter((item) => item !== code) : [...selected, code],
    );
  return (
    <fieldset className="grid gap-2 border-0 p-0" data-slot="ocr-languages" disabled={disabled}>
      <legend className={sectionTitleClass}>{copy.title}</legend>
      <Field label={copy.search} hint={copy.searchHint}>
        {({ id, invalid: _invalid, ...aria }) => (
          <input
            {...aria}
            id={id}
            type="search"
            autoComplete="off"
            spellCheck={false}
            className={`${controlClass} w-full`}
            value={query}
            onChange={(event) => setQuery(event.target.value)}
          />
        )}
      </Field>
      {/* biome-ignore lint/a11y/useSemanticElements: a labelled group of toggle buttons */}
      <div role="group" aria-label={copy.available} className="flex flex-wrap gap-1.5">
        {visible.map((language) => (
          <Chip
            key={language.code}
            pressed={selected.includes(language.code)}
            onClick={() => toggle(language.code)}
            label={`${language.name} (${language.code})`}
          >
            <span lang={language.code === "chi_sim" ? "zh-Hans" : undefined}>
              {language.native}
            </span>
            <span className="text-xs opacity-70">{language.code}</span>
          </Chip>
        ))}
        {visible.length === 0 ? (
          <p className="text-sm text-muted-foreground">{copy.empty}</p>
        ) : null}
      </div>
      <p className="text-sm text-start" data-slot="ocr-selected-languages">
        <span className="font-medium">{copy.selected}: </span>
        {selected.length === 0 ? (
          <span className="text-destructive">{copy.none}</span>
        ) : (
          <span>{selected.map((code) => languageName(code)).join(" + ")}</span>
        )}
      </p>
    </fieldset>
  );
}

/** Recognition studio for `pdf.ocr` and `image.ocr`. */
function OcrStudio({ tool, pageCount, disabled, onChange }: CustomToolPanelProps) {
  const isPdf = tool.id === "pdf.ocr";
  const [languages, setLanguages] = useState<string[]>(DEFAULT_OCR_LANGUAGES);
  const [mode, setMode] = useState<Mode>("skip-text");
  const [output, setOutput] = useState("pdf");
  const [pages, setPages] = useState("all");
  const [dpi, setDpi] = useState("300");
  const [deskew, setDeskew] = useState(false);
  const [autoRotate, setAutoRotate] = useState(false);
  const [clean, setClean] = useState(false);
  const dpiRange = bounds(tool, "dpi", isPdf ? [150, 600] : [70, 1200]);
  const dpiValue = numberDraft(dpi);

  const params = useMemo<Record<string, unknown>>(() => {
    const base: Record<string, unknown> = { languages, output };
    if (dpiValue.value !== undefined) base.dpi = dpiValue.value;
    if (!isPdf) return base;
    return { ...base, mode, pages, deskew, auto_rotate: autoRotate, clean };
  }, [languages, output, dpiValue.value, isPdf, mode, pages, deskew, autoRotate, clean]);
  const validation = useEmitParams(tool, params, {
    pageCount,
    extraErrors: {
      ...(languages.length === 0 ? { languages: ocrCopy.languages.none } : {}),
      ...(dpiValue.error ? { dpi: dpiValue.error } : {}),
    },
    onChange,
  });

  const outputs = isPdf
    ? [
        { value: "pdf", label: ocrCopy.output.pdf },
        { value: "pdf+txt", label: ocrCopy.output.pdfTxt },
        { value: "pdf+hocr", label: ocrCopy.output.pdfHocr },
      ]
    : [
        { value: "pdf", label: ocrCopy.output.imagePdf },
        { value: "txt", label: ocrCopy.output.txt },
        { value: "hocr", label: ocrCopy.output.hocr },
      ];
  const modes: { value: Mode; label: string; hint: string }[] = [
    { value: "skip-text", label: ocrCopy.existing.skip, hint: ocrCopy.existing.skipHint },
    { value: "force", label: ocrCopy.existing.force, hint: ocrCopy.existing.forceHint },
    { value: "redo", label: ocrCopy.existing.redo, hint: ocrCopy.existing.redoHint },
  ];

  return (
    <>
      <LanguagePicker selected={languages} onChange={setLanguages} disabled={disabled} />
      {isPdf ? (
        <>
          <fieldset className="grid gap-2 border-0 p-0" disabled={disabled}>
            <legend className={sectionTitleClass}>{ocrCopy.prep.title}</legend>
            <CheckRow
              label={ocrCopy.prep.deskew}
              hint={ocrCopy.prep.deskewHint}
              checked={deskew}
              onChange={setDeskew}
            />
            <CheckRow
              label={ocrCopy.prep.autoRotate}
              hint={ocrCopy.prep.autoRotateHint}
              checked={autoRotate}
              onChange={setAutoRotate}
            />
            <CheckRow
              label={ocrCopy.prep.clean}
              hint={ocrCopy.prep.cleanHint}
              checked={clean}
              onChange={setClean}
            />
          </fieldset>
          <div className="grid gap-1.5">
            <p className={sectionTitleClass}>{ocrCopy.existing.title}</p>
            <ChoiceChips
              label={ocrCopy.existing.title}
              options={modes}
              value={mode}
              onSelect={setMode}
              disabled={disabled}
            />
            <p className="text-xs text-muted-foreground text-start">
              {modes.find((item) => item.value === mode)?.hint}
            </p>
          </div>
          <PagesField
            value={pages}
            onChange={setPages}
            error={validation.errors.pages}
            disabled={disabled}
          />
        </>
      ) : null}
      <div className="grid gap-1.5">
        <p className={sectionTitleClass}>{ocrCopy.output.title}</p>
        <ChoiceChips
          label={ocrCopy.output.title}
          options={outputs}
          value={output}
          onSelect={setOutput}
          disabled={disabled}
        />
      </div>
      <NumberField
        label={ocrCopy.fields.dpi}
        hint={ocrCopy.fields.dpiHint(dpiRange[0], dpiRange[1])}
        draft={dpi}
        onDraft={setDpi}
        error={validation.errors.dpi}
        range={dpiRange}
        step={1}
        disabled={disabled}
      />
      <TextLayerPreview languages={languages} deskew={deskew} clean={clean} />
    </>
  );
}

/** Studio for `pdf.deskew`: how much tilt to detect and correct. */
function DeskewStudio({ tool, pageCount, disabled, onChange }: CustomToolPanelProps) {
  const [pages, setPages] = useState("all");
  const [dpi, setDpi] = useState("150");
  const [maxAngle, setMaxAngle] = useState("10");
  const [minAngle, setMinAngle] = useState("0.1");
  const dpiValue = numberDraft(dpi);
  const maxValue = numberDraft(maxAngle);
  const minValue = numberDraft(minAngle);
  const params = useMemo<Record<string, unknown>>(
    () => ({
      pages,
      ...(dpiValue.value === undefined ? {} : { dpi: dpiValue.value }),
      ...(maxValue.value === undefined ? {} : { max_angle_deg: maxValue.value }),
      ...(minValue.value === undefined ? {} : { min_angle_deg: minValue.value }),
    }),
    [pages, dpiValue.value, maxValue.value, minValue.value],
  );
  const validation = useEmitParams(tool, params, {
    pageCount,
    extraErrors: {
      ...(dpiValue.error ? { dpi: dpiValue.error } : {}),
      ...(maxValue.error ? { max_angle_deg: maxValue.error } : {}),
      ...(minValue.error ? { min_angle_deg: minValue.error } : {}),
    },
    onChange,
  });
  return (
    <>
      <p className="text-sm text-muted-foreground text-start">{ocrCopy.summary.deskew}</p>
      <PagesField
        value={pages}
        onChange={setPages}
        error={validation.errors.pages}
        disabled={disabled}
      />
      <NumberField
        label={ocrCopy.fields.dpi}
        draft={dpi}
        onDraft={setDpi}
        error={validation.errors.dpi}
        range={bounds(tool, "dpi", [72, 300])}
        step={1}
        disabled={disabled}
      />
      <NumberField
        label={ocrCopy.fields.maxAngle}
        draft={maxAngle}
        onDraft={setMaxAngle}
        error={validation.errors.max_angle_deg}
        range={bounds(tool, "max_angle_deg", [0.5, 45])}
        disabled={disabled}
      />
      <NumberField
        label={ocrCopy.fields.minAngle}
        draft={minAngle}
        onDraft={setMinAngle}
        error={validation.errors.min_angle_deg}
        range={bounds(tool, "min_angle_deg", [0, 5])}
        disabled={disabled}
      />
      <TextLayerPreview languages={DEFAULT_OCR_LANGUAGES} deskew clean={false} />
    </>
  );
}

/** Studio for `pdf.auto_rotate`: orientation detection resolution and confidence. */
function AutoRotateStudio({ tool, pageCount, disabled, onChange }: CustomToolPanelProps) {
  const [pages, setPages] = useState("all");
  const [dpi, setDpi] = useState("300");
  const [confidence, setConfidence] = useState("2");
  const dpiValue = numberDraft(dpi);
  const confidenceValue = numberDraft(confidence);
  const params = useMemo<Record<string, unknown>>(
    () => ({
      pages,
      ...(dpiValue.value === undefined ? {} : { dpi: dpiValue.value }),
      ...(confidenceValue.value === undefined ? {} : { min_confidence: confidenceValue.value }),
    }),
    [pages, dpiValue.value, confidenceValue.value],
  );
  const validation = useEmitParams(tool, params, {
    pageCount,
    extraErrors: {
      ...(dpiValue.error ? { dpi: dpiValue.error } : {}),
      ...(confidenceValue.error ? { min_confidence: confidenceValue.error } : {}),
    },
    onChange,
  });
  return (
    <>
      <p className="text-sm text-muted-foreground text-start">{ocrCopy.summary.autoRotate}</p>
      <PagesField
        value={pages}
        onChange={setPages}
        error={validation.errors.pages}
        disabled={disabled}
      />
      <NumberField
        label={ocrCopy.fields.dpi}
        draft={dpi}
        onDraft={setDpi}
        error={validation.errors.dpi}
        range={bounds(tool, "dpi", [150, 600])}
        step={1}
        disabled={disabled}
      />
      <NumberField
        label={ocrCopy.fields.minConfidence}
        hint={ocrCopy.fields.minConfidenceHint}
        draft={confidence}
        onDraft={setConfidence}
        error={validation.errors.min_confidence}
        range={bounds(tool, "min_confidence", [0, 100])}
        disabled={disabled}
      />
    </>
  );
}

/** Purpose-built studio for the OCR and scan tools; each tool remounts its own draft. */
export function OcrPanel(props: CustomToolPanelProps) {
  const { id } = props.tool;
  return (
    <div key={id} data-slot="ocr-panel" data-tool={id} className="grid gap-4 text-start">
      {id === "pdf.deskew" ? (
        <DeskewStudio {...props} />
      ) : id === "pdf.auto_rotate" ? (
        <AutoRotateStudio {...props} />
      ) : (
        <OcrStudio {...props} />
      )}
    </div>
  );
}
