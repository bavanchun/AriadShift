import { type ParamsValidation, validatePageRange } from "../../params-form";

export const WATERMARK_TOOLS = [
  "pdf.watermark",
  "pdf.watermark_remove",
  "pdf.stamp",
  "pdf.page_numbers",
  "pdf.header_footer",
  "pdf.bates",
  "pdf.qr",
] as const;
export type WatermarkToolId = (typeof WATERMARK_TOOLS)[number];

export function isWatermarkTool(id: string): id is WatermarkToolId {
  return (WATERMARK_TOOLS as readonly string[]).includes(id);
}

/** The preview page, in PDF points (A4). The overlay maths runs in the same units as the engine. */
export const PAGE_WIDTH = 595;
export const PAGE_HEIGHT = 842;

export const ANCHORS = [
  "top-left",
  "top-center",
  "top-right",
  "middle-left",
  "center",
  "middle-right",
  "bottom-left",
  "bottom-center",
  "bottom-right",
] as const;
export type Anchor = (typeof ANCHORS)[number];

export const ANCHOR_LABELS: Record<Anchor, string> = {
  "top-left": "Top left",
  "top-center": "Top center",
  "top-right": "Top right",
  "middle-left": "Middle left",
  center: "Center",
  "middle-right": "Middle right",
  "bottom-left": "Bottom left",
  "bottom-center": "Bottom center",
  "bottom-right": "Bottom right",
};

export const SWATCHES = [
  "#808080",
  "#000000",
  "#b91c1c",
  "#c2410c",
  "#15803d",
  "#1d4ed8",
  "#7e22ce",
] as const;

export type WatermarkLayout = "single" | "tiled" | "diagonal";
export type StampPreset = "approved" | "draft" | "confidential" | "custom";
export type DateFormat = "iso" | "dmy" | "mdy";
export type QrLevel = "L" | "M" | "Q" | "H";

export interface Slots {
  left: string;
  center: string;
  right: string;
}

export interface OverlaySettings {
  anchor: Anchor;
  color: string;
  /** `null` keeps the tool's own colour (stamp presets). */
  colorOverride: boolean;
  fontSize: number;
  autoSize: boolean;
  opacity: number;
  rotation: number;
  offsetX: number;
  offsetY: number;
  margin: number;
  pages: string;
  // watermark
  text: string;
  layout: WatermarkLayout;
  layer: "over" | "under";
  // stamp
  preset: StampPreset;
  border: boolean;
  showDate: boolean;
  dateFormat: DateFormat;
  // page numbers
  format: string;
  start: number;
  skipFirst: number;
  // header / footer
  header: Slots;
  footer: Slots;
  // bates
  prefix: string;
  suffix: string;
  digits: number;
  // qr
  content: string;
  sizePt: number;
  ecLevel: QrLevel;
  qrBackground: "white" | "transparent";
  // watermark removal
  previewText: string;
  candidates: string;
}

const EMPTY_SLOTS: Slots = { left: "", center: "", right: "" };

export function defaultSettings(tool: WatermarkToolId): OverlaySettings {
  const base: OverlaySettings = {
    anchor: "bottom-center",
    color: "#000000",
    colorOverride: true,
    fontSize: 10,
    autoSize: false,
    opacity: 1,
    rotation: 0,
    offsetX: 0,
    offsetY: 0,
    margin: 24,
    pages: "all",
    text: "",
    layout: "diagonal",
    layer: "over",
    preset: "approved",
    border: true,
    showDate: false,
    dateFormat: "iso",
    format: "Page {n} of {total}",
    start: 1,
    skipFirst: 0,
    header: EMPTY_SLOTS,
    footer: { ...EMPTY_SLOTS, center: "Page {n} of {total}" },
    prefix: "BATES-",
    suffix: "",
    digits: 6,
    content: "",
    sizePt: 72,
    ecLevel: "M",
    qrBackground: "white",
    previewText: "CONFIDENTIAL",
    candidates: "",
  };
  switch (tool) {
    case "pdf.watermark":
      return {
        ...base,
        anchor: "center",
        color: "#808080",
        fontSize: 64,
        autoSize: true,
        opacity: 0.3,
        rotation: 45,
        margin: 36,
        text: "CONFIDENTIAL",
      };
    case "pdf.stamp":
      return {
        ...base,
        anchor: "top-right",
        colorOverride: false,
        fontSize: 24,
        opacity: 0.9,
        margin: 36,
        pages: "1",
      };
    case "pdf.bates":
      return { ...base, anchor: "bottom-right", fontSize: 9, margin: 18 };
    case "pdf.qr":
      return { ...base, anchor: "bottom-right", margin: 24, pages: "1" };
    case "pdf.header_footer":
      return { ...base, fontSize: 9 };
    default:
      return base;
  }
}

// ---- text helpers -----------------------------------------------------------------------------

const SAMPLE_DATES: Record<DateFormat, string> = {
  iso: "2026-12-31",
  dmy: "31/12/2026",
  mdy: "12/31/2026",
};

export function sampleDate(format: DateFormat): string {
  return SAMPLE_DATES[format];
}

/**
 * Expands `{n}`, `{total}` and `{label}`, and `{date}` only when `date` is given, the way the
 * preview page would show them. `pdf.page_numbers` accepts no `{date}`, so it passes none.
 */
export function expandTemplate(
  template: string,
  values: { n: number; total: number | string; date?: string },
): string {
  const expanded = template
    .replaceAll("{n}", String(values.n))
    .replaceAll("{total}", String(values.total))
    .replaceAll("{label}", String(values.n));
  return values.date === undefined ? expanded : expanded.replaceAll("{date}", values.date);
}

export function batesLabel(
  settings: Pick<OverlaySettings, "prefix" | "suffix" | "digits" | "start">,
) {
  return `${settings.prefix}${String(settings.start).padStart(settings.digits, "0")}${settings.suffix}`;
}

export function stampLabel(settings: OverlaySettings): string {
  const base = settings.preset === "custom" ? settings.text : settings.preset.toUpperCase();
  return settings.showDate ? `${base} ${sampleDate(settings.dateFormat)}`.trim() : base;
}

export const STAMP_COLORS: Record<Exclude<StampPreset, "custom">, string> = {
  approved: "#15803d",
  draft: "#b45309",
  confidential: "#b91c1c",
};

// ---- placement --------------------------------------------------------------------------------

export interface OverlayItem {
  key: string;
  text: string;
  x: number;
  y: number;
  size: number;
  rotation: number;
  opacity: number;
  color: string;
  textAnchor: "start" | "middle" | "end";
  border?: boolean;
}

/** Page position of an anchor, in preview units (y grows downwards), before the text baseline. */
export function anchorPoint(
  anchor: Anchor,
  margin: number,
  offsetX = 0,
  offsetY = 0,
): { x: number; y: number; textAnchor: OverlayItem["textAnchor"] } {
  const column = anchor.endsWith("left") ? 0 : anchor.endsWith("right") ? 2 : 1;
  const row = anchor.startsWith("top") ? 0 : anchor.startsWith("bottom") ? 2 : 1;
  const x = ([margin, PAGE_WIDTH / 2, PAGE_WIDTH - margin] as const)[column] ?? margin;
  const y = ([margin, PAGE_HEIGHT / 2, PAGE_HEIGHT - margin] as const)[row] ?? margin;
  // PDF offsets grow upwards, the preview downwards.
  return {
    x: x + offsetX,
    y: y - offsetY,
    textAnchor: (["start", "middle", "end"] as const)[column] ?? "middle",
  };
}

/** The baseline for text hanging below the top margin, centred in the middle, above the bottom. */
function baseline(anchor: Anchor, y: number, size: number): number {
  if (anchor.startsWith("top")) return y + size;
  if (anchor.startsWith("bottom")) return y;
  return y + size * 0.35;
}

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

export function watermarkSize(settings: OverlaySettings): number {
  if (!settings.autoSize) return settings.fontSize;
  const length = Math.max(settings.text.length, 1);
  return clamp(Math.round((PAGE_WIDTH * 0.8) / (length * 0.62)), 24, 96);
}

export function overlayItems(
  tool: WatermarkToolId,
  settings: OverlaySettings,
  pageCount?: number,
): OverlayItem[] {
  const total = pageCount ?? "N";
  const date = sampleDate(settings.dateFormat);
  const point = anchorPoint(settings.anchor, settings.margin, settings.offsetX, settings.offsetY);

  if (tool === "pdf.watermark") {
    if (!settings.text.trim()) return [];
    const size = watermarkSize(settings);
    const common = {
      size,
      rotation: settings.rotation,
      opacity: settings.opacity,
      color: settings.color,
    };
    if (settings.layout !== "tiled") {
      return [
        {
          key: "mark",
          text: settings.text,
          x: point.x,
          y: baseline(settings.anchor, point.y, size),
          textAnchor: point.textAnchor,
          ...common,
        },
      ];
    }
    const gap = 72;
    const stepX = settings.text.length * size * 0.55 + gap;
    const stepY = size + gap;
    const items: OverlayItem[] = [];
    for (let row = 0; row * stepY < PAGE_HEIGHT && items.length < 60; row += 1) {
      for (let col = 0; col * stepX < PAGE_WIDTH && items.length < 60; col += 1) {
        items.push({
          key: `tile-${row}-${col}`,
          text: settings.text,
          x: col * stepX + (row % 2) * (stepX / 2) + 20,
          y: row * stepY + size,
          textAnchor: "start",
          ...common,
        });
      }
    }
    return items;
  }

  if (tool === "pdf.stamp") {
    const label = stampLabel(settings);
    if (!label.trim()) return [];
    return [
      {
        key: "stamp",
        text: label,
        x: point.x,
        y: baseline(settings.anchor, point.y, settings.fontSize),
        size: settings.fontSize,
        rotation: settings.rotation,
        opacity: settings.opacity,
        color: settings.colorOverride
          ? settings.color
          : settings.preset === "custom"
            ? settings.color
            : STAMP_COLORS[settings.preset],
        textAnchor: point.textAnchor,
        border: settings.border,
      },
    ];
  }

  if (tool === "pdf.page_numbers") {
    const text = expandTemplate(settings.format, { n: settings.start, total });
    return text
      ? [
          {
            key: "number",
            text,
            x: point.x,
            y: baseline(settings.anchor, point.y, settings.fontSize),
            size: settings.fontSize,
            rotation: 0,
            opacity: 1,
            color: settings.color,
            textAnchor: point.textAnchor,
          },
        ]
      : [];
  }

  if (tool === "pdf.bates") {
    return [
      {
        key: "bates",
        text: batesLabel(settings),
        x: point.x,
        y: baseline(settings.anchor, point.y, settings.fontSize),
        size: settings.fontSize,
        rotation: 0,
        opacity: 1,
        color: settings.color,
        textAnchor: point.textAnchor,
      },
    ];
  }

  if (tool === "pdf.header_footer") {
    const items: OverlayItem[] = [];
    const m = settings.margin;
    const x = { left: m, center: PAGE_WIDTH / 2, right: PAGE_WIDTH - m };
    const anchorOf = { left: "start", center: "middle", right: "end" } as const;
    for (const [band, slots] of [
      ["header", settings.header],
      ["footer", settings.footer],
    ] as const) {
      for (const slot of ["left", "center", "right"] as const) {
        const raw = slots[slot];
        if (!raw) continue;
        items.push({
          key: `${band}-${slot}`,
          text: expandTemplate(raw, { n: 1, total, date }),
          x: x[slot],
          y: band === "header" ? m + settings.fontSize : PAGE_HEIGHT - m,
          size: settings.fontSize,
          rotation: 0,
          opacity: 1,
          color: settings.color,
          textAnchor: anchorOf[slot],
        });
      }
    }
    return items;
  }

  return [];
}

/** Top-left corner of the QR box on the preview page. */
export function qrBox(settings: OverlaySettings): { x: number; y: number; size: number } {
  const point = anchorPoint(settings.anchor, settings.margin, settings.offsetX, settings.offsetY);
  const size = settings.sizePt;
  const column = point.textAnchor === "start" ? 0 : point.textAnchor === "middle" ? 0.5 : 1;
  const row = settings.anchor.startsWith("top")
    ? 0
    : settings.anchor.startsWith("bottom")
      ? 1
      : 0.5;
  return { x: point.x - size * column, y: point.y - size * row, size };
}

// ---- parameters and validation ----------------------------------------------------------------

const QR_MAX_BYTES = 2953;

export function parseCandidates(text: string): string[] {
  return text
    .split(/[\s,]+/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function slotsParam(slots: Slots): Record<string, string | null> {
  return {
    left: slots.left || null,
    center: slots.center || null,
    right: slots.right || null,
  };
}

export function buildParams(tool: WatermarkToolId, s: OverlaySettings): Record<string, unknown> {
  switch (tool) {
    case "pdf.watermark":
      return {
        kind: "text",
        text: s.text,
        anchor: s.anchor,
        color: s.color,
        font_size_pt: s.autoSize ? "auto" : s.fontSize,
        layout: s.layout,
        layer: s.layer,
        margin_pt: s.margin,
        offset_x_pt: s.offsetX,
        offset_y_pt: s.offsetY,
        opacity: s.opacity,
        pages: s.pages,
        rotation_deg: s.rotation,
      };
    case "pdf.stamp":
      return {
        preset: s.preset,
        ...(s.preset === "custom" ? { text: s.text } : {}),
        anchor: s.anchor,
        ...(s.colorOverride ? { color: s.color } : {}),
        border: s.border,
        font_size_pt: s.fontSize,
        margin_pt: s.margin,
        offset_x_pt: s.offsetX,
        offset_y_pt: s.offsetY,
        opacity: s.opacity,
        pages: s.pages,
        rotation_deg: s.rotation,
        show_date: s.showDate,
        date_format: s.dateFormat,
      };
    case "pdf.page_numbers":
      return {
        anchor: s.anchor,
        color: s.color,
        font_size_pt: s.fontSize,
        format: s.format,
        margin_pt: s.margin,
        offset_x_pt: s.offsetX,
        offset_y_pt: s.offsetY,
        pages: s.pages,
        skip_first: s.skipFirst,
        start: s.start,
      };
    case "pdf.header_footer":
      return {
        color: s.color,
        font_size_pt: s.fontSize,
        header: slotsParam(s.header),
        footer: slotsParam(s.footer),
        margin_pt: s.margin,
        pages: s.pages,
        skip_first: s.skipFirst,
      };
    case "pdf.bates":
      return {
        anchor: s.anchor,
        color: s.color,
        digits: s.digits,
        font_size_pt: s.fontSize,
        margin_pt: s.margin,
        offset_x_pt: s.offsetX,
        offset_y_pt: s.offsetY,
        pages: s.pages,
        prefix: s.prefix,
        start: s.start,
        suffix: s.suffix,
      };
    case "pdf.qr":
      return {
        anchor: s.anchor,
        background: s.qrBackground,
        color: s.color,
        content: s.content,
        ec_level: s.ecLevel,
        margin_pt: s.margin,
        offset_x_pt: s.offsetX,
        offset_y_pt: s.offsetY,
        pages: s.pages,
        size_pt: s.sizePt,
      };
    case "pdf.watermark_remove":
      return { candidates: [...new Set(parseCandidates(s.candidates))] };
  }
}

export function validateSettings(
  tool: WatermarkToolId,
  s: OverlaySettings,
  pageCount?: number,
): ParamsValidation {
  const errors: Record<string, string> = {};
  if (tool === "pdf.watermark_remove") {
    const candidates = parseCandidates(s.candidates);
    if (candidates.length === 0) errors.candidates = "Enter at least one candidate identifier.";
    else if (candidates.some((item) => item.includes("*") || item.includes("?")))
      errors.candidates = "Wildcards are not accepted; list each identifier.";
    return { isValid: Object.keys(errors).length === 0, errors };
  }
  const pageError = validatePageRange(s.pages, pageCount);
  if (pageError) errors.pages = pageError;
  if (tool === "pdf.watermark" && !s.text.trim()) errors.text = "Enter the watermark text.";
  if (tool === "pdf.stamp" && s.preset === "custom" && !s.text.trim())
    errors.text = "Enter the stamp text.";
  if (tool === "pdf.page_numbers" && !s.format.trim()) errors.format = "Enter a number format.";
  if (tool === "pdf.qr") {
    const bytes = new TextEncoder().encode(s.content).length;
    if (bytes === 0) errors.content = "Enter the text or link the code should hold.";
    else if (bytes > QR_MAX_BYTES) errors.content = `Too long: at most ${QR_MAX_BYTES} bytes.`;
  }
  if (tool === "pdf.header_footer") {
    const any = [...Object.values(s.header), ...Object.values(s.footer)].some((slot) =>
      slot.trim(),
    );
    if (!any) errors.header = "Fill in at least one header or footer slot.";
  }
  return { isValid: Object.keys(errors).length === 0, errors };
}
