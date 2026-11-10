"use client";

import { Alert, AlertDescription, AlertTitle } from "@ariadshift/ui/components/alert";
import { Button } from "@ariadshift/ui/components/button";
import { ShieldAlertIcon, XIcon } from "lucide-react";
import { type KeyboardEvent, useMemo, useState } from "react";
import {
  CheckRow,
  Chip,
  type CustomToolPanelProps,
  controlClass,
  ErrorText,
  Field,
  sectionTitleClass,
  useEmitParams,
} from "../ocr/panel-kit";
import { redactCopy } from "./copy";
import {
  EMPTY_RECT,
  formatRect,
  MAX_PATTERNS,
  OVERLAY_LABELS,
  PATTERN_CHIPS,
  PRESET_COLORS,
  parseRect,
  type RectDraft,
  type RectTarget,
  type RedactionTarget,
  validateRegex,
  validateTerm,
} from "./targets";
import { RedactionVerificationCard } from "./verification-card";

export const REDACT_TOOLS = ["pdf.redact", "pdf.redact_verify"] as const;
export type RedactToolId = (typeof REDACT_TOOLS)[number];

export function isRedactTool(id: string): id is RedactToolId {
  return (REDACT_TOOLS as readonly string[]).includes(id);
}

const SCOPE_KEYS = ["text", "images", "vectors", "annotations", "metadata", "attachments"] as const;
type ScopeKey = (typeof SCOPE_KEYS)[number];

const onEnter = (action: () => void) => (event: KeyboardEvent<HTMLInputElement>) => {
  if (event.key !== "Enter") return;
  event.preventDefault();
  action();
};

/** A list of removable entries, each a button that takes the entry out. */
function EntryList({
  label,
  entries,
  onRemove,
  disabled,
}: {
  label: string;
  entries: { key: string; text: string }[];
  onRemove: (index: number) => void;
  disabled?: boolean;
}) {
  if (entries.length === 0) return null;
  return (
    <ul aria-label={label} className="flex flex-wrap gap-1.5">
      {entries.map((entry, index) => (
        <li
          key={entry.key}
          className="inline-flex max-w-full items-center gap-1 rounded-full border border-input bg-muted ps-3 text-sm"
        >
          <span className="truncate font-mono text-xs">{entry.text}</span>
          <button
            type="button"
            disabled={disabled}
            aria-label={redactCopy.targets.remove(entry.text)}
            onClick={() => onRemove(index)}
            className="inline-flex size-7 items-center justify-center rounded-full outline-none hover:bg-background focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50"
          >
            <XIcon aria-hidden="true" className="size-3.5" />
          </button>
        </li>
      ))}
    </ul>
  );
}

function AddRow({
  label,
  hint,
  value,
  onValue,
  error,
  onAdd,
  addDisabled,
  disabled,
  mono,
}: {
  label: string;
  hint?: string;
  value: string;
  onValue: (value: string) => void;
  error?: string | null;
  onAdd: () => void;
  addDisabled: boolean;
  disabled?: boolean;
  mono?: boolean;
}) {
  return (
    <Field label={label} hint={hint} error={error ?? undefined}>
      {({ id, invalid: _invalid, ...aria }) => (
        <div className="flex gap-2">
          <input
            {...aria}
            id={id}
            type="text"
            autoComplete="off"
            spellCheck={false}
            className={`${controlClass} w-full ${mono ? "font-mono" : ""}`}
            disabled={disabled}
            value={value}
            onChange={(event) => onValue(event.target.value)}
            onKeyDown={onEnter(() => {
              if (!addDisabled) onAdd();
            })}
          />
          <Button
            type="button"
            variant="outline"
            disabled={disabled || addDisabled}
            onClick={onAdd}
          >
            {redactCopy.targets.add}
          </Button>
        </div>
      )}
    </Field>
  );
}

const rectFields = ["page", "left", "bottom", "right", "top"] as const;

/** Studio for `pdf.redact` and `pdf.redact_verify`: choose what to remove and how it looks. */
function RedactStudio({ tool, pageCount, disabled, onChange }: CustomToolPanelProps) {
  const verifyOnly = tool.id === "pdf.redact_verify";
  const copy = redactCopy;
  const [terms, setTerms] = useState<string[]>([]);
  const [termDraft, setTermDraft] = useState("");
  const [chips, setChips] = useState<string[]>([]);
  const [regexOn, setRegexOn] = useState(false);
  const [regexes, setRegexes] = useState<string[]>([]);
  const [regexDraft, setRegexDraft] = useState("");
  const [rects, setRects] = useState<RectTarget[]>([]);
  const [rectDraft, setRectDraft] = useState<RectDraft>(EMPTY_RECT);
  const [color, setColor] = useState<string>(PRESET_COLORS[0]);
  const [overlay, setOverlay] = useState("");
  const [scope, setScope] = useState<Record<ScopeKey, boolean>>({
    text: true,
    images: true,
    vectors: true,
    annotations: true,
    metadata: true,
    attachments: true,
  });
  const [invalidate, setInvalidate] = useState(false);

  const termError = validateTerm(termDraft);
  const regexError = validateRegex(regexDraft.trim());
  const rectResult = parseRect(rectDraft, pageCount);

  const targets = useMemo<RedactionTarget[]>(
    () => [
      ...terms.map((text): RedactionTarget => ({ kind: "exact", text })),
      ...PATTERN_CHIPS.filter((chip) => chips.includes(chip.id)).flatMap((chip) => chip.targets),
      ...regexes.map((pattern): RedactionTarget => ({ kind: "regex", pattern })),
      ...rects,
    ],
    [terms, chips, regexes, rects],
  );
  const patternCount = targets.length - rects.length;

  const params = useMemo<Record<string, unknown>>(() => {
    const base = { targets, scope };
    if (verifyOnly) return base;
    return {
      ...base,
      color,
      ...(overlay.trim() ? { overlay_text: overlay.trim() } : {}),
      invalidate_signatures: invalidate,
    };
  }, [targets, scope, verifyOnly, color, overlay, invalidate]);

  const validation = useEmitParams(tool, params, {
    extraErrors: {
      ...(targets.length === 0 ? { targets: copy.targets.none } : {}),
      ...(patternCount > MAX_PATTERNS ? { targets: copy.targets.tooMany(MAX_PATTERNS) } : {}),
    },
    onChange,
  });

  const addTerm = () => {
    const text = termDraft.trim();
    if (!text || termError || terms.includes(text)) return;
    setTerms([...terms, text]);
    setTermDraft("");
  };
  const addRegex = () => {
    const pattern = regexDraft.trim();
    if (!pattern || regexError || regexes.includes(pattern)) return;
    setRegexes([...regexes, pattern]);
    setRegexDraft("");
  };
  const addRect = () => {
    if (!("target" in rectResult)) return;
    setRects([...rects, rectResult.target]);
    setRectDraft(EMPTY_RECT);
  };
  const toggleChip = (id: string) =>
    setChips(chips.includes(id) ? chips.filter((item) => item !== id) : [...chips, id]);

  return (
    <>
      {verifyOnly ? null : (
        <Alert variant="destructive" data-slot="redaction-notice">
          <ShieldAlertIcon aria-hidden="true" />
          <AlertTitle>{copy.notice.title}</AlertTitle>
          <AlertDescription>{copy.notice.body}</AlertDescription>
        </Alert>
      )}

      <section className="grid gap-3" aria-labelledby="redact-targets-title">
        <h3 id="redact-targets-title" className={sectionTitleClass}>
          {copy.targets.title}
        </h3>
        <AddRow
          label={copy.targets.termInput}
          hint={copy.targets.termHint}
          value={termDraft}
          onValue={setTermDraft}
          error={termError}
          onAdd={addTerm}
          addDisabled={!termDraft.trim() || !!termError}
          disabled={disabled}
        />
        <EntryList
          label={copy.targets.terms}
          entries={terms.map((text) => ({ key: text, text }))}
          onRemove={(index) => setTerms(terms.filter((_, at) => at !== index))}
          disabled={disabled}
        />

        <div className="grid gap-1.5">
          <p className="text-sm font-medium">{copy.targets.patterns}</p>
          {/* biome-ignore lint/a11y/useSemanticElements: a labelled group of toggle buttons */}
          <div role="group" aria-label={copy.targets.patterns} className="flex flex-wrap gap-1.5">
            {PATTERN_CHIPS.map((chip) => (
              <Chip
                key={chip.id}
                pressed={chips.includes(chip.id)}
                disabled={disabled}
                onClick={() => toggleChip(chip.id)}
              >
                {chip.label}
              </Chip>
            ))}
            <Chip pressed={regexOn} disabled={disabled} onClick={() => setRegexOn(!regexOn)}>
              {copy.targets.customRegex}
            </Chip>
          </div>
        </div>

        {regexOn ? (
          <div className="grid gap-2" data-slot="redact-regex">
            <AddRow
              label={copy.targets.regexInput}
              hint={copy.targets.regexHint}
              value={regexDraft}
              onValue={setRegexDraft}
              error={regexError}
              onAdd={addRegex}
              addDisabled={!regexDraft.trim() || !!regexError}
              disabled={disabled}
              mono
            />
            {regexDraft.trim() && !regexError ? (
              <p className="text-xs text-muted-foreground text-start" data-slot="regex-valid">
                {copy.targets.regexOk}
              </p>
            ) : null}
            <EntryList
              label={copy.targets.customRegex}
              entries={regexes.map((text) => ({ key: text, text }))}
              onRemove={(index) => setRegexes(regexes.filter((_, at) => at !== index))}
              disabled={disabled}
            />
          </div>
        ) : null}

        <fieldset className="grid gap-2 border-0 p-0" disabled={disabled}>
          <legend className="text-sm font-medium">{copy.targets.areas}</legend>
          <p className="text-xs text-muted-foreground text-start">{copy.targets.areasHint}</p>
          <div className="grid grid-cols-5 gap-2">
            {rectFields.map((field) => (
              <Field key={field} label={copy.targets[field]}>
                {({ id, invalid: _invalid, ...aria }) => (
                  <input
                    {...aria}
                    id={id}
                    type="number"
                    inputMode="decimal"
                    step={field === "page" ? 1 : "any"}
                    min={field === "page" ? 1 : undefined}
                    className={`${controlClass} w-full`}
                    value={rectDraft[field]}
                    onChange={(event) =>
                      setRectDraft({ ...rectDraft, [field]: event.target.value })
                    }
                    onKeyDown={onEnter(addRect)}
                  />
                )}
              </Field>
            ))}
          </div>
          {"error" in rectResult ? <ErrorText>{rectResult.error ?? undefined}</ErrorText> : null}
          <div>
            <Button
              type="button"
              variant="outline"
              disabled={disabled || !("target" in rectResult)}
              onClick={addRect}
            >
              {copy.targets.addArea}
            </Button>
          </div>
          <EntryList
            label={copy.targets.areas}
            entries={rects.map((rect) => ({ key: formatRect(rect), text: formatRect(rect) }))}
            onRemove={(index) => setRects(rects.filter((_, at) => at !== index))}
            disabled={disabled}
          />
        </fieldset>

        <p
          className={`text-sm text-start ${validation.errors.targets ? "text-destructive" : "text-muted-foreground"}`}
          data-slot="redact-summary"
        >
          {validation.errors.targets ?? copy.targets.summary(targets.length)}
        </p>
      </section>

      {verifyOnly ? null : (
        <section className="grid gap-3" aria-labelledby="redact-look-title">
          <h3 id="redact-look-title" className={sectionTitleClass}>
            {copy.appearance.title}
          </h3>
          <div className="grid gap-1.5">
            <p className="text-sm font-medium">{copy.appearance.fill}</p>
            <div className="flex flex-wrap items-center gap-2">
              {PRESET_COLORS.map((preset) => (
                <button
                  key={preset}
                  type="button"
                  disabled={disabled}
                  aria-pressed={color === preset}
                  aria-label={copy.appearance.fillPreset(preset)}
                  onClick={() => setColor(preset)}
                  style={{ backgroundColor: preset }}
                  className="size-8 rounded-full border border-input outline-none focus-visible:ring-3 focus-visible:ring-ring/50 aria-pressed:ring-3 aria-pressed:ring-primary disabled:opacity-50"
                />
              ))}
              <input
                type="color"
                aria-label={copy.appearance.custom}
                disabled={disabled}
                value={color}
                onChange={(event) => setColor(event.target.value)}
                className="h-8 w-12 cursor-pointer rounded-md border border-input bg-background p-0.5"
              />
              <span className="font-mono text-xs text-muted-foreground" data-slot="redact-color">
                {color}
              </span>
            </div>
          </div>
          <div className="grid gap-1.5">
            <Field label={copy.appearance.label}>
              {({ id, invalid: _invalid, ...aria }) => (
                <input
                  {...aria}
                  id={id}
                  type="text"
                  autoComplete="off"
                  className={`${controlClass} w-full`}
                  disabled={disabled}
                  value={overlay}
                  placeholder={copy.appearance.labelCustom}
                  onChange={(event) => setOverlay(event.target.value)}
                />
              )}
            </Field>
            {/* biome-ignore lint/a11y/useSemanticElements: a labelled group of toggle buttons */}
            <div
              role="group"
              aria-label={copy.appearance.presets}
              className="flex flex-wrap gap-1.5"
            >
              <Chip pressed={overlay === ""} disabled={disabled} onClick={() => setOverlay("")}>
                {copy.appearance.labelNone}
              </Chip>
              {OVERLAY_LABELS.map((label) => (
                <Chip
                  key={label}
                  pressed={overlay === label}
                  disabled={disabled}
                  onClick={() => setOverlay(label)}
                >
                  {label}
                </Chip>
              ))}
            </div>
          </div>
        </section>
      )}

      <fieldset className="grid gap-2 border-0 p-0" disabled={disabled}>
        <legend className={sectionTitleClass}>{copy.scope.title}</legend>
        {SCOPE_KEYS.map((key) => (
          <CheckRow
            key={key}
            label={copy.scope[key]}
            checked={scope[key]}
            onChange={(checked) => setScope({ ...scope, [key]: checked })}
          />
        ))}
        {SCOPE_KEYS.some((key) => !scope[key]) ? (
          <p className="text-xs text-muted-foreground text-start">{copy.scope.partial}</p>
        ) : null}
        {verifyOnly ? null : (
          <CheckRow
            label={copy.signatures.label}
            hint={copy.signatures.hint}
            checked={invalidate}
            onChange={setInvalidate}
          />
        )}
      </fieldset>

      <RedactionVerificationCard standalone={verifyOnly} />
    </>
  );
}

/** Purpose-built studio for the redaction tools. */
export function RedactPanel(props: CustomToolPanelProps) {
  const { id } = props.tool;
  return (
    <div key={id} data-slot="redact-panel" data-tool={id} className="grid gap-4 text-start">
      <RedactStudio {...props} />
    </div>
  );
}
