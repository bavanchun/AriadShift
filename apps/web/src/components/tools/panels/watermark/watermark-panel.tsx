"use client";

import { cn } from "@ariadshift/ui/lib/utils";
import { type ReactNode, useCallback, useId, useMemo, useState } from "react";
import {
  CONTROL_CLASS,
  type CustomToolPanelProps,
  useEmitParams,
} from "../compress/panel-contract";
import { AnchorGrid } from "./anchor-grid";
import { OverlayPreview } from "./overlay-preview";
import { RemovalStudio } from "./removal-studio";
import {
  buildParams,
  type DateFormat,
  defaultSettings,
  isWatermarkTool,
  type OverlaySettings,
  type QrLevel,
  type StampPreset,
  SWATCHES,
  validateSettings,
  type WatermarkLayout,
  type WatermarkToolId,
} from "./watermark-model";

const NUDGE_PT = 10;
const STAMP_CHIPS: ReadonlyArray<{ label: string; preset: StampPreset; text?: string }> = [
  { label: "CONFIDENTIAL", preset: "confidential" },
  { label: "DRAFT", preset: "draft" },
  { label: "APPROVED", preset: "approved" },
  { label: "COPY", preset: "custom", text: "COPY" },
];
const NUMBER_FORMATS = ["{n}", "Page {n} of {total}", "{n} / {total}", "- {n} -"];
const LAYOUTS: ReadonlyArray<{ value: WatermarkLayout; label: string }> = [
  { value: "single", label: "Single" },
  { value: "tiled", label: "Tiled" },
  { value: "diagonal", label: "Diagonal" },
];

export function WatermarkPanel(props: CustomToolPanelProps) {
  return <WatermarkDraft key={props.tool.id} {...props} />;
}

function WatermarkDraft({ tool, files, pageCount, disabled, onChange }: CustomToolPanelProps) {
  const id = isWatermarkTool(tool.id) ? tool.id : "pdf.watermark";
  const [settings, setSettings] = useState<OverlaySettings>(() => defaultSettings(id));
  const pages = pageCount ?? files[0]?.pageCount ?? undefined;
  const patch = useCallback(
    (change: Partial<OverlaySettings>) => setSettings((previous) => ({ ...previous, ...change })),
    [],
  );

  const params = useMemo(() => buildParams(id, settings), [id, settings]);
  const checked = useMemo(() => validateSettings(id, settings, pages), [id, settings, pages]);
  useEmitParams(params, checked, onChange);

  if (id === "pdf.watermark_remove") {
    return (
      <RemovalStudio
        settings={settings}
        onChange={patch}
        error={checked.errors.candidates}
        pageCount={pages}
        disabled={disabled}
      />
    );
  }

  const errors = checked.errors;
  const positioned = id !== "pdf.header_footer";
  const rotates = id === "pdf.watermark" || id === "pdf.stamp";
  const fades = rotates;

  return (
    <div
      data-slot="watermark-panel"
      className="grid gap-5 md:grid-cols-[minmax(0,18rem)_minmax(0,1fr)]"
    >
      <div className="grid content-start justify-items-center gap-2 md:sticky md:top-2 md:self-start">
        <OverlayPreview
          tool={id}
          settings={settings}
          pageCount={pages}
          onOffset={
            positioned && !disabled ? (x, y) => patch({ offsetX: x, offsetY: y }) : undefined
          }
        />
        {positioned ? (
          <p className="text-center text-xs text-muted-foreground">
            Drag on the page to move the mark.
          </p>
        ) : null}
      </div>

      <div className="grid content-start gap-4">
        {id === "pdf.watermark" ? (
          <>
            <TextField
              label="Watermark text"
              value={settings.text}
              onValue={(text) => patch({ text })}
              error={errors.text}
              disabled={disabled}
            />
            <ToggleRow
              label="Layout"
              value={settings.layout}
              options={LAYOUTS}
              onValue={(layout) => patch({ layout })}
              disabled={disabled}
            />
            <ToggleRow
              label="Layer"
              value={settings.layer}
              options={[
                { value: "over", label: "Over content" },
                { value: "under", label: "Under content" },
              ]}
              onValue={(layer) => patch({ layer })}
              disabled={disabled}
            />
          </>
        ) : null}

        {id === "pdf.stamp" ? (
          <StampControls
            settings={settings}
            patch={patch}
            error={errors.text}
            disabled={disabled}
          />
        ) : null}

        {id === "pdf.page_numbers" ? (
          <>
            <TextField
              label="Number format"
              value={settings.format}
              onValue={(format) => patch({ format })}
              error={errors.format}
              disabled={disabled}
            />
            <div className="flex flex-wrap gap-1.5">
              {NUMBER_FORMATS.map((format) => (
                <Chip
                  key={format}
                  active={settings.format === format}
                  onClick={() => patch({ format })}
                  disabled={disabled}
                >
                  {format}
                </Chip>
              ))}
            </div>
            <div className="grid grid-cols-2 gap-3">
              <NumberField
                label="Start at"
                value={settings.start}
                min={0}
                onValue={(start) => patch({ start })}
                disabled={disabled}
              />
              <NumberField
                label="Skip first pages"
                value={settings.skipFirst}
                min={0}
                onValue={(skipFirst) => patch({ skipFirst })}
                disabled={disabled}
              />
            </div>
          </>
        ) : null}

        {id === "pdf.header_footer" ? (
          <HeaderFooterControls
            settings={settings}
            patch={patch}
            error={errors.header}
            disabled={disabled}
          />
        ) : null}

        {id === "pdf.bates" ? (
          <div className="grid grid-cols-2 gap-3">
            <TextField
              label="Prefix"
              value={settings.prefix}
              onValue={(prefix) => patch({ prefix })}
              disabled={disabled}
            />
            <TextField
              label="Suffix"
              value={settings.suffix}
              onValue={(suffix) => patch({ suffix })}
              disabled={disabled}
            />
            <NumberField
              label="Digits"
              value={settings.digits}
              min={1}
              max={12}
              onValue={(digits) => patch({ digits })}
              disabled={disabled}
            />
            <NumberField
              label="Start at"
              value={settings.start}
              min={0}
              onValue={(start) => patch({ start })}
              disabled={disabled}
            />
          </div>
        ) : null}

        {id === "pdf.qr" ? (
          <QrControls
            settings={settings}
            patch={patch}
            error={errors.content}
            disabled={disabled}
          />
        ) : null}

        {positioned ? (
          <div className="flex flex-wrap items-start gap-5">
            <AnchorGrid
              value={settings.anchor}
              onChange={(anchor) => patch({ anchor, offsetX: 0, offsetY: 0 })}
              disabled={disabled}
            />
            <OffsetControls settings={settings} patch={patch} disabled={disabled} />
          </div>
        ) : null}

        {rotates ? (
          <RangeField
            label="Rotation"
            unit="°"
            min={-90}
            max={90}
            value={settings.rotation}
            onValue={(rotation) => patch({ rotation })}
            disabled={disabled}
          />
        ) : null}
        {fades ? (
          <RangeField
            label="Opacity"
            unit="%"
            min={10}
            max={100}
            value={Math.round(settings.opacity * 100)}
            onValue={(percent) => patch({ opacity: percent / 100 })}
            disabled={disabled}
          />
        ) : null}

        {id !== "pdf.qr" ? (
          <div className="grid grid-cols-2 gap-3">
            <NumberField
              label="Font size (pt)"
              value={settings.fontSize}
              min={4}
              max={400}
              onValue={(fontSize) => patch({ fontSize, autoSize: false })}
              disabled={disabled}
            />
            {id === "pdf.watermark" ? (
              <label className="flex cursor-pointer items-end gap-2 pb-2 text-sm">
                <input
                  type="checkbox"
                  className="size-4 cursor-pointer accent-primary"
                  checked={settings.autoSize}
                  disabled={disabled}
                  onChange={(event) => patch({ autoSize: event.target.checked })}
                />
                Fit the text to the page
              </label>
            ) : null}
          </div>
        ) : null}

        <Swatches
          value={settings.colorOverride ? settings.color : null}
          onValue={(color) =>
            patch(color === null ? { colorOverride: false } : { color, colorOverride: true })
          }
          allowPreset={id === "pdf.stamp"}
          disabled={disabled}
        />

        <TextField
          label="Pages"
          value={settings.pages}
          onValue={(value) => patch({ pages: value })}
          error={errors.pages}
          hint="For example all, odd, 1, 2-5 or last"
          disabled={disabled}
        />
      </div>
    </div>
  );
}

// ---- tool-specific groups ---------------------------------------------------------------------

function StampControls({
  settings,
  patch,
  error,
  disabled,
}: {
  settings: OverlaySettings;
  patch: (change: Partial<OverlaySettings>) => void;
  error?: string;
  disabled?: boolean;
}) {
  const isCopy = settings.preset === "custom" && settings.text === "COPY";
  return (
    <>
      <fieldset className="grid gap-1.5" disabled={disabled}>
        <legend className="mb-1 text-sm font-medium">Stamp</legend>
        <div className="flex flex-wrap gap-1.5">
          {STAMP_CHIPS.map((chip) => (
            <Chip
              key={chip.label}
              active={chip.text ? isCopy : settings.preset === chip.preset}
              onClick={() => patch({ preset: chip.preset, text: chip.text ?? settings.text })}
              disabled={disabled}
            >
              {chip.label}
            </Chip>
          ))}
          <Chip
            active={settings.preset === "custom" && !isCopy}
            onClick={() => patch({ preset: "custom", text: isCopy ? "" : settings.text })}
            disabled={disabled}
          >
            Custom
          </Chip>
        </div>
      </fieldset>
      {settings.preset === "custom" ? (
        <TextField
          label="Stamp text"
          value={settings.text}
          onValue={(text) => patch({ text })}
          error={error}
          disabled={disabled}
        />
      ) : null}
      <div className="flex flex-wrap items-center gap-4 text-sm">
        <label className="flex cursor-pointer items-center gap-2">
          <input
            type="checkbox"
            className="size-4 cursor-pointer accent-primary"
            checked={settings.border}
            disabled={disabled}
            onChange={(event) => patch({ border: event.target.checked })}
          />
          Border
        </label>
        <label className="flex cursor-pointer items-center gap-2">
          <input
            type="checkbox"
            className="size-4 cursor-pointer accent-primary"
            checked={settings.showDate}
            disabled={disabled}
            onChange={(event) => patch({ showDate: event.target.checked })}
          />
          Add the date
        </label>
        {settings.showDate ? (
          <select
            aria-label="Date format"
            className={cn(CONTROL_CLASS, "h-9")}
            disabled={disabled}
            value={settings.dateFormat}
            onChange={(event) => patch({ dateFormat: event.target.value as DateFormat })}
          >
            <option value="iso">2026-12-31</option>
            <option value="dmy">31/12/2026</option>
            <option value="mdy">12/31/2026</option>
          </select>
        ) : null}
      </div>
    </>
  );
}

function HeaderFooterControls({
  settings,
  patch,
  error,
  disabled,
}: {
  settings: OverlaySettings;
  patch: (change: Partial<OverlaySettings>) => void;
  error?: string;
  disabled?: boolean;
}) {
  const slots = ["left", "center", "right"] as const;
  const names = { left: "left", center: "centre", right: "right" } as const;
  return (
    <>
      {(["header", "footer"] as const).map((band) => (
        <fieldset key={band} className="grid gap-1.5" disabled={disabled}>
          <legend className="mb-1 text-sm font-medium capitalize">{band}</legend>
          <div className="grid gap-2 sm:grid-cols-3">
            {slots.map((slot) => (
              <TextField
                key={slot}
                label={`${band === "header" ? "Header" : "Footer"} ${names[slot]}`}
                value={settings[band][slot]}
                onValue={(value) => patch({ [band]: { ...settings[band], [slot]: value } })}
                disabled={disabled}
              />
            ))}
          </div>
        </fieldset>
      ))}
      <p className="text-xs text-muted-foreground">
        Use {"{n}"} for the page number, {"{total}"} for the page count and {"{date}"} for the date.
      </p>
      {error ? (
        <p role="alert" className="text-sm text-destructive">
          {error}
        </p>
      ) : null}
      <NumberField
        label="Skip first pages"
        value={settings.skipFirst}
        min={0}
        onValue={(skipFirst) => patch({ skipFirst })}
        disabled={disabled}
      />
    </>
  );
}

function QrControls({
  settings,
  patch,
  error,
  disabled,
}: {
  settings: OverlaySettings;
  patch: (change: Partial<OverlaySettings>) => void;
  error?: string;
  disabled?: boolean;
}) {
  const prefix = useId();
  return (
    <>
      <div className="grid gap-1.5">
        <label htmlFor={`${prefix}-content`} className="text-sm font-medium">
          QR payload
        </label>
        <textarea
          id={`${prefix}-content`}
          className={cn(CONTROL_CLASS, "min-h-20 w-full py-2 text-sm")}
          spellCheck={false}
          disabled={disabled}
          aria-invalid={!!error}
          value={settings.content}
          onChange={(event) => patch({ content: event.target.value })}
        />
        {error ? (
          <p role="alert" className="text-sm text-destructive">
            {error}
          </p>
        ) : null}
      </div>
      <RangeField
        label="Size"
        unit=" pt"
        min={18}
        max={300}
        value={settings.sizePt}
        onValue={(sizePt) => patch({ sizePt })}
        disabled={disabled}
      />
      <div className="grid grid-cols-2 gap-3 text-sm">
        <label className="grid gap-1.5 font-medium">
          Error correction
          <select
            className={cn(CONTROL_CLASS, "h-9 font-normal")}
            disabled={disabled}
            value={settings.ecLevel}
            onChange={(event) => patch({ ecLevel: event.target.value as QrLevel })}
          >
            <option value="L">Low (7%)</option>
            <option value="M">Medium (15%)</option>
            <option value="Q">Quartile (25%)</option>
            <option value="H">High (30%)</option>
          </select>
        </label>
        <label className="grid gap-1.5 font-medium">
          Background
          <select
            className={cn(CONTROL_CLASS, "h-9 font-normal")}
            disabled={disabled}
            value={settings.qrBackground}
            onChange={(event) =>
              patch({ qrBackground: event.target.value as "white" | "transparent" })
            }
          >
            <option value="white">White</option>
            <option value="transparent">Transparent</option>
          </select>
        </label>
      </div>
    </>
  );
}

function OffsetControls({
  settings,
  patch,
  disabled,
}: {
  settings: OverlaySettings;
  patch: (change: Partial<OverlaySettings>) => void;
  disabled?: boolean;
}) {
  const nudge = (dx: number, dy: number) =>
    patch({ offsetX: settings.offsetX + dx, offsetY: settings.offsetY + dy });
  const button =
    "inline-flex h-8 cursor-pointer items-center justify-center rounded-md border border-input bg-background px-2 text-xs outline-none hover:bg-muted focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50";
  return (
    <fieldset className="grid min-w-0 flex-1 gap-2" disabled={disabled}>
      <legend className="mb-1 text-sm font-medium">Offset (pt)</legend>
      <div className="grid grid-cols-2 gap-3">
        <NumberField
          label="Horizontal offset"
          value={settings.offsetX}
          onValue={(offsetX) => patch({ offsetX })}
          disabled={disabled}
        />
        <NumberField
          label="Vertical offset"
          value={settings.offsetY}
          onValue={(offsetY) => patch({ offsetY })}
          disabled={disabled}
        />
      </div>
      <div className="flex flex-wrap gap-1.5" dir="ltr">
        <button type="button" className={button} onClick={() => nudge(-NUDGE_PT, 0)}>
          Nudge left
        </button>
        <button type="button" className={button} onClick={() => nudge(NUDGE_PT, 0)}>
          Nudge right
        </button>
        <button type="button" className={button} onClick={() => nudge(0, NUDGE_PT)}>
          Nudge up
        </button>
        <button type="button" className={button} onClick={() => nudge(0, -NUDGE_PT)}>
          Nudge down
        </button>
      </div>
    </fieldset>
  );
}

// ---- small controls ---------------------------------------------------------------------------

function Chip({
  active,
  onClick,
  disabled,
  children,
}: {
  active: boolean;
  onClick: () => void;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "cursor-pointer rounded-full border px-3 py-1 text-xs font-medium outline-none transition-colors focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50",
        active ? "border-primary bg-primary/10" : "border-input bg-background hover:bg-muted",
      )}
    >
      {children}
    </button>
  );
}

function ToggleRow<T extends string>({
  label,
  value,
  options,
  onValue,
  disabled,
}: {
  label: string;
  value: T;
  options: ReadonlyArray<{ value: T; label: string }>;
  onValue: (value: T) => void;
  disabled?: boolean;
}) {
  return (
    <fieldset className="grid gap-1.5" disabled={disabled}>
      <legend className="mb-1 text-sm font-medium">{label}</legend>
      <div className="flex flex-wrap gap-1.5">
        {options.map((option) => (
          <Chip
            key={option.value}
            active={value === option.value}
            onClick={() => onValue(option.value)}
            disabled={disabled}
          >
            {option.label}
          </Chip>
        ))}
      </div>
    </fieldset>
  );
}

function TextField({
  label,
  value,
  onValue,
  error,
  hint,
  disabled,
}: {
  label: string;
  value: string;
  onValue: (value: string) => void;
  error?: string;
  hint?: string;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <div className="grid gap-1.5">
      <label htmlFor={id} className="text-sm font-medium">
        {label}
      </label>
      <input
        id={id}
        type="text"
        autoComplete="off"
        spellCheck={false}
        disabled={disabled}
        aria-invalid={!!error}
        aria-describedby={error ? `${id}-error` : undefined}
        className={cn(CONTROL_CLASS, "h-9 w-full")}
        value={value}
        onChange={(event) => onValue(event.target.value)}
      />
      {error ? (
        <p id={`${id}-error`} role="alert" className="text-sm text-destructive">
          {error}
        </p>
      ) : hint ? (
        <p className="text-xs text-muted-foreground">{hint}</p>
      ) : null}
    </div>
  );
}

function NumberField({
  label,
  value,
  min,
  max,
  onValue,
  disabled,
}: {
  label: string;
  value: number;
  min?: number;
  max?: number;
  onValue: (value: number) => void;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <div className="grid gap-1.5">
      <label htmlFor={id} className="text-sm font-medium">
        {label}
      </label>
      <input
        id={id}
        type="number"
        min={min}
        max={max}
        step="any"
        disabled={disabled}
        className={cn(CONTROL_CLASS, "h-9 w-full")}
        value={value}
        onChange={(event) => {
          const next = event.target.valueAsNumber;
          if (Number.isFinite(next)) onValue(next);
        }}
      />
    </div>
  );
}

function RangeField({
  label,
  unit,
  min,
  max,
  value,
  onValue,
  disabled,
}: {
  label: string;
  unit: string;
  min: number;
  max: number;
  value: number;
  onValue: (value: number) => void;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <div className="grid gap-1.5">
      <label htmlFor={id} className="flex justify-between text-sm font-medium">
        <span>{label}</span>
        <output htmlFor={id}>
          {value}
          {unit}
        </output>
      </label>
      <input
        id={id}
        type="range"
        min={min}
        max={max}
        step={1}
        disabled={disabled}
        className="w-full accent-primary"
        value={value}
        onChange={(event) => onValue(Number(event.target.value))}
      />
    </div>
  );
}

function Swatches({
  value,
  onValue,
  allowPreset,
  disabled,
}: {
  value: string | null;
  onValue: (color: string | null) => void;
  allowPreset?: boolean;
  disabled?: boolean;
}) {
  return (
    <fieldset className="grid gap-1.5" disabled={disabled}>
      <legend className="mb-1 text-sm font-medium">Colour</legend>
      <div className="flex flex-wrap items-center gap-2">
        {allowPreset ? (
          <Chip active={value === null} onClick={() => onValue(null)} disabled={disabled}>
            Preset colour
          </Chip>
        ) : null}
        {SWATCHES.map((color) => (
          <button
            key={color}
            type="button"
            aria-label={`Colour ${color}`}
            aria-pressed={value === color}
            disabled={disabled}
            onClick={() => onValue(color)}
            style={{ backgroundColor: color }}
            className={cn(
              "size-7 cursor-pointer rounded-full border-2 outline-none focus-visible:ring-3 focus-visible:ring-ring/50 disabled:cursor-not-allowed disabled:opacity-50",
              value === color ? "border-primary ring-2 ring-primary/40" : "border-input",
            )}
          />
        ))}
      </div>
    </fieldset>
  );
}

export type { WatermarkToolId };
