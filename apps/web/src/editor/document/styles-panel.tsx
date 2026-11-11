"use client";

import { Button } from "@ariadshift/ui/components/button";
import { type ReactNode, useEffect, useId, useState } from "react";
import type { DocumentModeLabels } from "./document-mode-labels";
import type { DocumentEditSession, SessionSnapshot } from "./document-session";
import { findBlock, getInlines, TEXT_BLOCKS } from "./document-tree";
import { characterStyleAt, characterStyleIdAt, inlineLength } from "./inline-ops";
import type { CharacterStyle, ParagraphStyle } from "./mapping";
import { resolveParagraphStyle } from "./style-utils";

const fieldClass = "h-7 w-full rounded-md border border-border bg-background px-2 text-sm";
const COLOR_PATTERN = /^#[0-9a-f]{3,8}$/i;

function Field({ label, children }: { label: string; children: ReactNode }) {
  return (
    // biome-ignore lint/a11y/noLabelWithoutControl: the control is passed in as children
    <label className="flex flex-col gap-0.5 text-xs text-muted-foreground">
      {label}
      {children}
    </label>
  );
}

function TextField({
  label,
  value,
  placeholder,
  onChange,
}: {
  label: string;
  value: string | undefined;
  placeholder?: string;
  onChange: (value: string | undefined) => void;
}) {
  return (
    <Field label={label}>
      <input
        type="text"
        value={value ?? ""}
        placeholder={placeholder}
        className={fieldClass}
        onChange={(event) => onChange(event.target.value || undefined)}
      />
    </Field>
  );
}

/** A number box that keeps in-progress text such as "1." while the model holds the parsed value. */
function NumberField({
  label,
  value,
  placeholder,
  min,
  step,
  onChange,
}: {
  label: string;
  value: number | undefined;
  placeholder?: string;
  min?: number;
  step?: number;
  onChange: (value: number | undefined) => void;
}) {
  const [text, setText] = useState(value === undefined ? "" : String(value));
  useEffect(() => {
    setText((current) => {
      if (value === undefined) return current.trim() === "" ? current : "";
      return Number(current) === value ? current : String(value);
    });
  }, [value]);
  return (
    <Field label={label}>
      <input
        type="number"
        value={text}
        min={min}
        step={step}
        placeholder={placeholder}
        className={fieldClass}
        onChange={(event) => {
          const next = event.target.value;
          setText(next);
          if (next.trim() === "") onChange(undefined);
          else if (Number.isFinite(Number(next))) onChange(Number(next));
        }}
      />
    </Field>
  );
}

function ColorField({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string | undefined;
  onChange: (value: string | undefined) => void;
}) {
  const [text, setText] = useState(value ?? "");
  useEffect(() => setText(value ?? ""), [value]);
  return (
    <Field label={label}>
      <span className="flex items-center gap-1">
        <input
          type="text"
          value={text}
          placeholder="#000000"
          aria-label={label}
          className={fieldClass}
          onChange={(event) => {
            const next = event.target.value.trim();
            setText(event.target.value);
            if (next === "") onChange(undefined);
            else if (COLOR_PATTERN.test(next)) onChange(next);
          }}
        />
        <span
          aria-hidden="true"
          className="size-5 shrink-0 rounded border border-border"
          style={{ backgroundColor: value }}
        />
      </span>
    </Field>
  );
}

function Check({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (value: boolean | undefined) => void;
}) {
  return (
    <label className="flex items-center gap-2 text-sm">
      <input
        type="checkbox"
        checked={checked}
        onChange={(event) => onChange(event.target.checked || undefined)}
      />
      {label}
    </label>
  );
}

function ParagraphFields({
  labels,
  value,
  resolved,
  onPatch,
}: {
  labels: DocumentModeLabels;
  value: ParagraphStyle;
  resolved?: ParagraphStyle;
  onPatch: (patch: ParagraphStyle) => void;
}) {
  const shown = resolved ?? value;
  return (
    <div className="grid grid-cols-2 gap-2">
      <TextField
        label={labels.fontFamily}
        value={value.font?.family}
        placeholder={shown.font?.family}
        onChange={(family) => onPatch({ font: family ? { family } : undefined })}
      />
      <NumberField
        label={labels.fontSize}
        value={value.size_pt}
        placeholder={shown.size_pt === undefined ? undefined : String(shown.size_pt)}
        min={1}
        step={0.5}
        onChange={(size_pt) => onPatch({ size_pt })}
      />
      <NumberField
        label={labels.lineHeight}
        value={value.line_height}
        placeholder={shown.line_height === undefined ? undefined : String(shown.line_height)}
        min={0.1}
        step={0.05}
        onChange={(line_height) => onPatch({ line_height })}
      />
      <Field label={labels.alignment}>
        <select
          value={value.align ?? ""}
          className={fieldClass}
          onChange={(event) =>
            onPatch({ align: (event.target.value || undefined) as ParagraphStyle["align"] })
          }
        >
          <option value="">{shown.align ?? "left"}</option>
          <option value="left">{labels.alignLeft}</option>
          <option value="center">{labels.alignCenter}</option>
          <option value="right">{labels.alignRight}</option>
          <option value="justify">{labels.alignJustify}</option>
        </select>
      </Field>
      <NumberField
        label={labels.spaceBefore}
        value={value.space_before_pt}
        placeholder={
          shown.space_before_pt === undefined ? undefined : String(shown.space_before_pt)
        }
        min={0}
        onChange={(space_before_pt) => onPatch({ space_before_pt })}
      />
      <NumberField
        label={labels.spaceAfter}
        value={value.space_after_pt}
        placeholder={shown.space_after_pt === undefined ? undefined : String(shown.space_after_pt)}
        min={0}
        onChange={(space_after_pt) => onPatch({ space_after_pt })}
      />
      <ColorField
        label={labels.textColor}
        value={value.color}
        onChange={(color) => onPatch({ color })}
      />
      <ColorField
        label={labels.backgroundColor}
        value={value.background}
        onChange={(background) => onPatch({ background })}
      />
      <div className="col-span-2 flex flex-col gap-1">
        <Check
          label={labels.keepWithNext}
          checked={value.keep_with_next === true}
          onChange={(keep_with_next) => onPatch({ keep_with_next })}
        />
        <Check
          label={labels.pageBreakBefore}
          checked={value.page_break_before === true}
          onChange={(page_break_before) => onPatch({ page_break_before })}
        />
      </div>
    </div>
  );
}

function CharacterFields({
  labels,
  value,
  onPatch,
  full = false,
}: {
  labels: DocumentModeLabels;
  value: CharacterStyle;
  onPatch: (patch: CharacterStyle) => void;
  /** Weight, italic and underline are part of named styles; toolbar marks cover them for text. */
  full?: boolean;
}) {
  return (
    <div className="grid grid-cols-2 gap-2">
      <TextField
        label={labels.fontFamily}
        value={value.font?.family}
        onChange={(family) => onPatch({ font: family ? { family } : undefined })}
      />
      <NumberField
        label={labels.fontSize}
        value={value.size_pt}
        min={1}
        step={0.5}
        onChange={(size_pt) => onPatch({ size_pt })}
      />
      <ColorField
        label={labels.textColor}
        value={value.color}
        onChange={(color) => onPatch({ color })}
      />
      <ColorField
        label={labels.backgroundColor}
        value={value.highlight}
        onChange={(highlight) => onPatch({ highlight })}
      />
      {full && (
        <>
          <NumberField
            label={labels.weight}
            value={value.weight}
            min={100}
            step={100}
            onChange={(weight) => onPatch({ weight })}
          />
          <div className="flex flex-col gap-1">
            <Check
              label={labels.italicStyle}
              checked={value.italic === true}
              onChange={(italic) => onPatch({ italic })}
            />
            <Check
              label={labels.underlineStyle}
              checked={value.underline === true}
              onChange={(underline) => onPatch({ underline })}
            />
          </div>
        </>
      )}
    </div>
  );
}

export interface StylesPanelProps {
  session: DocumentEditSession;
  snapshot: SessionSnapshot;
  labels: DocumentModeLabels;
  className?: string;
}

/** Inspector for the selected block and editor for the document's named styles. */
export function StylesPanel({ session, snapshot, labels, className }: StylesPanelProps) {
  const headingId = useId();
  const { state, selectedBlockId, selection } = snapshot;
  const styles = state.original.styles;
  const selected = selectedBlockId ? findBlock(state.doc, selectedBlockId) : null;
  const isText = Boolean(selected && TEXT_BLOCKS.has(selected.node.type));
  const attrs = selected?.node.attrs ?? {};
  const styleId = typeof attrs.styleId === "string" ? attrs.styleId : "";
  const direct = (attrs.direct ?? {}) as ParagraphStyle;
  const resolved = resolveParagraphStyle(styles, styleId || null, direct);
  const paragraphIds = Object.keys(styles?.paragraph ?? {});
  const characterIds = Object.keys(styles?.character ?? {});

  const target =
    selection ??
    (selectedBlockId
      ? { blockId: selectedBlockId, part: "content" as const, from: 0, to: 0 }
      : null);
  const targetNodes = target
    ? (getInlines(findBlock(state.doc, target.blockId)?.node ?? { type: "" }, target.part) ?? null)
    : null;
  const range =
    targetNodes && target
      ? target.from === target.to
        ? { from: 0, to: inlineLength(targetNodes) }
        : target
      : null;
  const characterStyle =
    targetNodes && range ? characterStyleAt(targetNodes, range.from, range.to, styles) : {};
  const characterStyleId =
    targetNodes && range ? characterStyleIdAt(targetNodes, range.from, range.to) : null;

  const [editingParagraph, setEditingParagraph] = useState("");
  const [editingCharacter, setEditingCharacter] = useState("");
  const [newName, setNewName] = useState("");
  const [newKind, setNewKind] = useState<"paragraph" | "character">("paragraph");
  const paragraphDefinition = styles?.paragraph?.[editingParagraph];
  const characterDefinition = styles?.character?.[editingCharacter];

  const createStyle = () => {
    const name = newName.trim();
    if (!name) return;
    if (newKind === "paragraph") {
      session.updateParagraphStyle(name, {});
      setEditingParagraph(name);
    } else {
      session.updateCharacterStyle(name, {});
      setEditingCharacter(name);
    }
    setNewName("");
  };

  return (
    <aside
      aria-labelledby={headingId}
      className={`flex w-80 shrink-0 flex-col gap-4 overflow-y-auto border-s border-border bg-card p-3 ${className ?? ""}`}
    >
      <h2 id={headingId} className="text-sm font-semibold">
        {labels.stylesTitle}
      </h2>

      <section aria-label={labels.selectedBlock} className="flex flex-col gap-2">
        <h3 className="text-xs font-semibold uppercase text-muted-foreground">
          {labels.selectedBlock}
        </h3>
        {!isText ? (
          <p className="text-sm text-muted-foreground">{labels.noBlockSelected}</p>
        ) : (
          <>
            <Field label={labels.paragraphStyle}>
              <select
                value={styleId}
                className={fieldClass}
                onChange={(event) => session.setBlockStyle(event.target.value || null)}
              >
                <option value="">{labels.noNamedStyle}</option>
                {paragraphIds.map((id) => (
                  <option key={id} value={id}>
                    {id}
                  </option>
                ))}
              </select>
            </Field>
            <h4 className="text-xs font-medium">{labels.directFormatting}</h4>
            <ParagraphFields
              labels={labels}
              value={direct}
              resolved={resolved}
              onPatch={(patch) => session.setBlockDirect(patch)}
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => session.clearBlockDirect()}
            >
              {labels.resetDirect}
            </Button>
          </>
        )}
      </section>

      {isText && (
        <section aria-label={labels.characterFormatting} className="flex flex-col gap-2">
          <h3 className="text-xs font-semibold uppercase text-muted-foreground">
            {labels.characterFormatting}
          </h3>
          <Field label={labels.characterStyle}>
            <select
              value={characterStyleId ?? ""}
              className={fieldClass}
              onChange={(event) => session.applyCharacterStyle(event.target.value || null)}
            >
              <option value="">{labels.noCharacterStyle}</option>
              {characterIds.map((id) => (
                <option key={id} value={id}>
                  {id}
                </option>
              ))}
            </select>
          </Field>
          <CharacterFields
            labels={labels}
            value={characterStyle}
            onPatch={(patch) => session.applyCharacterDirect(patch)}
          />
        </section>
      )}

      <section aria-label={labels.documentStyles} className="flex flex-col gap-2">
        <h3 className="text-xs font-semibold uppercase text-muted-foreground">
          {labels.documentStyles}
        </h3>
        <Field label={labels.editParagraphStyle}>
          <select
            value={editingParagraph}
            className={fieldClass}
            onChange={(event) => setEditingParagraph(event.target.value)}
          >
            <option value="" />
            {paragraphIds.map((id) => (
              <option key={id} value={id}>
                {id}
              </option>
            ))}
          </select>
        </Field>
        {paragraphDefinition && (
          <>
            <Field label={labels.basedOn}>
              <select
                value={paragraphDefinition.based_on ?? ""}
                className={fieldClass}
                onChange={(event) =>
                  session.updateParagraphStyle(editingParagraph, {
                    based_on: event.target.value || undefined,
                  })
                }
              >
                <option value="" />
                {paragraphIds
                  .filter((id) => id !== editingParagraph)
                  .map((id) => (
                    <option key={id} value={id}>
                      {id}
                    </option>
                  ))}
              </select>
            </Field>
            <ParagraphFields
              labels={labels}
              value={paragraphDefinition}
              onPatch={(patch) => session.updateParagraphStyle(editingParagraph, patch)}
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              aria-label={`${labels.deleteStyle} ${editingParagraph}`}
              onClick={() => {
                if (session.deleteParagraphStyle(editingParagraph)) setEditingParagraph("");
              }}
            >
              {labels.deleteStyle}
            </Button>
          </>
        )}
        <Field label={labels.editCharacterStyle}>
          <select
            value={editingCharacter}
            className={fieldClass}
            onChange={(event) => setEditingCharacter(event.target.value)}
          >
            <option value="" />
            {characterIds.map((id) => (
              <option key={id} value={id}>
                {id}
              </option>
            ))}
          </select>
        </Field>
        {characterDefinition && (
          <CharacterFields
            full
            labels={labels}
            value={characterDefinition}
            onPatch={(patch) => session.updateCharacterStyle(editingCharacter, patch)}
          />
        )}
        <div className="flex items-end gap-2">
          <Field label={labels.newStyleName}>
            <input
              type="text"
              value={newName}
              className={fieldClass}
              onChange={(event) => setNewName(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  createStyle();
                }
              }}
            />
          </Field>
          <select
            aria-label={labels.createStyle}
            value={newKind}
            className="h-7 rounded-md border border-border bg-background px-1 text-sm"
            onChange={(event) => setNewKind(event.target.value as "paragraph" | "character")}
          >
            <option value="paragraph">{labels.paragraphStyle}</option>
            <option value="character">{labels.characterStyle}</option>
          </select>
          <Button type="button" size="sm" disabled={!newName.trim()} onClick={createStyle}>
            {labels.createStyle}
          </Button>
        </div>
      </section>
    </aside>
  );
}
