// Page mode data model: the objects the in-place editor shows, and the font fallback chain that
// decides what happens when replacement text does not fit the PDF's embedded font.

/** Left, bottom, right, top, in unrotated PDF page points (origin at the bottom left). */
export type PageBBox = readonly [left: number, bottom: number, right: number, top: number];

export interface TextRunObject {
  kind: "text";
  id: string;
  /** One-based page number. */
  page: number;
  bbox: PageBBox;
  text: string;
  fontName: string;
  fontSize: number;
  /** `#RRGGBB`. */
  color: string;
  /** Every glyph the embedded font program carries; only meaningful for a subset font. */
  embeddedGlyphs?: string;
  /** The font program only carries the glyphs the document used. */
  isSubset?: boolean;
}

export interface ImageObject {
  kind: "image";
  id: string;
  page: number;
  bbox: PageBBox;
  /** Pixel size of the source bitmap. */
  width: number;
  height: number;
  dataUrl?: string;
}

export interface PathObject {
  kind: "path";
  id: string;
  page: number;
  bbox: PageBBox;
  strokeColor: string;
  fillColor: string;
}

export type PageObject = TextRunObject | ImageObject | PathObject;

/** Page size in points, as reported by `edit.page_objects`. */
export interface PageInfo {
  page: number;
  widthPt: number;
  heightPt: number;
}

export const NOTO_VIETNAMESE_STACK = '"Noto Sans", "Plus Jakarta Sans", sans-serif';

export type FontTier = "embedded-subset" | "system-match" | "noto-vietnamese";

export interface FontResolution {
  tier: FontTier;
  /** CSS font family (or the embedded font's own name) the edited text is set in. */
  resolvedFont: string;
  /** Human-readable name of the resolved font, for the substitution warning. */
  resolvedLabel: string;
  /** Human-readable name of the font the run used before the edit. */
  originalLabel: string;
  substituted: boolean;
  /** Characters of the edited text the embedded subset cannot draw, in first-seen order. */
  missingChars: string[];
  /** The edited text after Unicode NFC normalisation: what is actually written to the PDF. */
  text: string;
  reason?: string;
}

const SUBSET_PREFIX = /^[A-Z]{6}\+/;

/** `ABCDEF+Helvetica` → `Subset Helvetica`; a full font keeps its name. */
export function displayFontName(fontName: string, isSubset: boolean | undefined): string {
  const name = fontName.replace(SUBSET_PREFIX, "");
  return isSubset || SUBSET_PREFIX.test(fontName) ? `Subset ${name}` : name;
}

type SystemFamily = "Serif" | "Mono" | "Sans";

function systemFamily(fontName: string): SystemFamily {
  if (/mono|courier|consolas/i.test(fontName)) return "Mono";
  if (/times|georgia|garamond|palatino|minion|serif/i.test(fontName) && !/sans/i.test(fontName)) {
    return "Serif";
  }
  return "Sans";
}

function missingGlyphs(text: string, glyphs: string | undefined): string[] {
  const available = new Set(Array.from((glyphs ?? "").normalize("NFC")));
  const missing = new Set<string>();
  for (const char of text) {
    if (/\s/u.test(char) || available.has(char)) continue;
    missing.add(char);
  }
  return [...missing];
}

/**
 * Picks the font for replacement text in three tiers: the run's own embedded font (when it is not
 * a subset, or the subset still carries every glyph), a system-matching Latin family, and finally
 * a Noto Sans stack with full Vietnamese coverage. The text is NFC-normalised first so a
 * decomposed `e` + combining marks never reaches the PDF as separate glyphs.
 */
export function resolveFontForEdit(object: TextRunObject, nextText: string): FontResolution {
  const text = nextText.normalize("NFC");
  const originalLabel = displayFontName(object.fontName, object.isSubset);
  const embedded: FontResolution = {
    tier: "embedded-subset",
    resolvedFont: object.fontName,
    resolvedLabel: originalLabel,
    originalLabel,
    substituted: false,
    missingChars: [],
    text,
  };
  if (!object.isSubset) return embedded;

  const missingChars = missingGlyphs(text, object.embeddedGlyphs);
  if (missingChars.length === 0) return embedded;

  const latinOnly = missingChars.every((char) => (char.codePointAt(0) ?? 0) <= 0xff);
  if (latinOnly) {
    const family = systemFamily(object.fontName);
    return {
      ...embedded,
      tier: "system-match",
      resolvedFont: `Liberation ${family}`,
      resolvedLabel: `Liberation ${family}`,
      substituted: true,
      missingChars,
      reason: "The embedded subset lacks some Latin characters; a matching system font is used.",
    };
  }
  return {
    ...embedded,
    tier: "noto-vietnamese",
    resolvedFont: NOTO_VIETNAMESE_STACK,
    resolvedLabel: "Noto Sans (Vietnamese coverage)",
    substituted: true,
    missingChars,
    reason:
      "The embedded subset lacks characters outside Latin-1; Noto Sans with full Vietnamese coverage is used.",
  };
}
