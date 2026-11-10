// Redaction targets as the `pdf.redact` schema defines them, plus the checks the panel runs
// before a value is accepted into the list.
export type RedactionPresetId = "email" | "phone" | "cccd" | "cmnd" | "iban" | "credit_card";

export interface RectTarget {
  kind: "rectangle";
  page: number;
  rect: { left: number; bottom: number; right: number; top: number };
}

export type RedactionTarget =
  | { kind: "exact"; text: string }
  | { kind: "regex"; pattern: string }
  | { kind: "preset"; preset: RedactionPresetId }
  | RectTarget;

/** The engine accepts at most this many text patterns (terms, regexes and detectors) per request. */
export const MAX_PATTERNS = 100;
export const MAX_PATTERN_BYTES = 1024;

export interface PatternChip {
  id: string;
  label: string;
  /** What the chip adds to the list while it is on. */
  targets: RedactionTarget[];
}

export const SSN_REGEX = String.raw`\b\d{3}-\d{2}-\d{4}\b`;
export const DATE_REGEX = String.raw`\b(?:\d{1,2}[/.-]\d{1,2}[/.-]\d{2,4}|\d{4}-\d{2}-\d{2})\b`;

/** SSN and dates have no built-in detector, so they go out as ordinary regex targets. */
export const PATTERN_CHIPS: readonly PatternChip[] = [
  { id: "email", label: "Email", targets: [{ kind: "preset", preset: "email" }] },
  { id: "phone", label: "Phone", targets: [{ kind: "preset", preset: "phone" }] },
  { id: "credit_card", label: "Credit Card", targets: [{ kind: "preset", preset: "credit_card" }] },
  {
    id: "national_id",
    label: "SSN / National ID",
    targets: [
      { kind: "preset", preset: "cccd" },
      { kind: "preset", preset: "cmnd" },
      { kind: "regex", pattern: SSN_REGEX },
    ],
  },
  { id: "iban", label: "IBAN", targets: [{ kind: "preset", preset: "iban" }] },
  { id: "date", label: "Date", targets: [{ kind: "regex", pattern: DATE_REGEX }] },
];

const utf8Length = (text: string) => new TextEncoder().encode(text).length;

/**
 * Checks a custom pattern. The browser compiles it as a quick syntax check, but the engine uses a
 * linear-time regex dialect, so lookaround and back-references are refused up front.
 */
export function validateRegex(pattern: string): string | null {
  if (!pattern) return null;
  if (utf8Length(pattern) > MAX_PATTERN_BYTES)
    return `Use at most ${MAX_PATTERN_BYTES} bytes per pattern.`;
  try {
    new RegExp(pattern, "u");
  } catch (error) {
    return `Invalid regular expression: ${error instanceof Error ? error.message : "syntax error"}`;
  }
  if (/\(\?<?[=!]/.test(pattern)) return "Lookahead and lookbehind are not supported.";
  if (/\\[1-9]|\\k</.test(pattern)) return "Back-references are not supported.";
  return null;
}

export function validateTerm(text: string): string | null {
  if (!text.trim()) return null;
  return utf8Length(text) > MAX_PATTERN_BYTES
    ? `Use at most ${MAX_PATTERN_BYTES} bytes per term.`
    : null;
}

export interface RectDraft {
  page: string;
  left: string;
  bottom: string;
  right: string;
  top: string;
}

export const EMPTY_RECT: RectDraft = { page: "", left: "", bottom: "", right: "", top: "" };

/** A rectangle draft turned into a target, or the reason it cannot be one yet. */
export function parseRect(
  draft: RectDraft,
  pageCount?: number,
): { target: RectTarget } | { error: string | null } {
  const values = [draft.page, draft.left, draft.bottom, draft.right, draft.top];
  if (values.every((value) => value.trim() === "")) return { error: null };
  if (values.some((value) => value.trim() === "")) return { error: "Fill in all five values." };
  const [page, left, bottom, right, top] = values.map(Number) as [
    number,
    number,
    number,
    number,
    number,
  ];
  if (![page, left, bottom, right, top].every(Number.isFinite))
    return { error: "Enter numbers only." };
  if (!Number.isInteger(page) || page < 1)
    return { error: "The page must be a whole number from 1." };
  if (pageCount !== undefined && page > pageCount)
    return { error: `The document has ${pageCount} pages.` };
  if ([left, bottom, right, top].some((value) => Math.abs(value) > 1_000_000))
    return { error: "Coordinates must stay within 1,000,000 points." };
  if (left >= right) return { error: "Left must be smaller than right." };
  if (bottom >= top) return { error: "Bottom must be smaller than top." };
  return { target: { kind: "rectangle", page, rect: { left, bottom, right, top } } };
}

export const formatRect = (target: RectTarget) =>
  `[${target.page}, ${target.rect.left}, ${target.rect.bottom}, ${target.rect.right}, ${target.rect.top}]`;

export const PRESET_COLORS = ["#000000", "#ffffff", "#dc2626"] as const;
export const OVERLAY_LABELS = ["REDACTED", "CONFIDENTIAL"] as const;
