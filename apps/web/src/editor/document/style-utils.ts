import type { CSSProperties } from "react";
import type { CharacterStyle, FontRef, ParagraphStyle, StyleSheet } from "./mapping";

const MAX_STYLE_CHAIN = 16;

function familyList(font: FontRef | undefined): string | undefined {
  if (!font?.family) return undefined;
  return [font.family, ...(font.fallback ?? [])]
    .map((name) => `"${name.replace(/["\\]/g, "")}"`)
    .join(", ");
}

/** Resolves `based_on` inheritance and then the direct overrides of one block. */
export function resolveParagraphStyle(
  styles: StyleSheet | undefined,
  styleId: string | null | undefined,
  direct?: ParagraphStyle,
): ParagraphStyle {
  const chain: ParagraphStyle[] = [];
  const seen = new Set<string>();
  let id = styleId ?? undefined;
  while (id && !seen.has(id) && chain.length < MAX_STYLE_CHAIN) {
    seen.add(id);
    const style = styles?.paragraph?.[id];
    if (!style) break;
    chain.unshift(style);
    id = style.based_on;
  }
  return Object.assign({}, ...chain, direct ?? {});
}

export function resolveCharacterStyle(
  styles: StyleSheet | undefined,
  styleId: string | null | undefined,
  direct?: CharacterStyle,
): CharacterStyle {
  const base = styleId ? styles?.character?.[styleId] : undefined;
  return { ...base, ...direct };
}

export function paragraphCss(style: ParagraphStyle): CSSProperties {
  return {
    fontFamily: familyList(style.font),
    fontSize: style.size_pt === undefined ? undefined : `${style.size_pt}pt`,
    lineHeight: style.line_height,
    marginBlockStart:
      style.space_before_pt === undefined ? undefined : `${style.space_before_pt}pt`,
    marginBlockEnd: style.space_after_pt === undefined ? undefined : `${style.space_after_pt}pt`,
    textAlign: style.align,
    textIndent: style.indent_first_pt === undefined ? undefined : `${style.indent_first_pt}pt`,
    paddingInlineStart:
      style.indent_left_pt === undefined ? undefined : `${style.indent_left_pt}pt`,
    paddingInlineEnd:
      style.indent_right_pt === undefined ? undefined : `${style.indent_right_pt}pt`,
    color: style.color,
    backgroundColor: style.background,
  };
}

export function characterCss(style: CharacterStyle): CSSProperties {
  const decorations = [style.underline ? "underline" : "", style.strike ? "line-through" : ""]
    .filter(Boolean)
    .join(" ");
  return {
    fontFamily: familyList(style.font),
    fontSize: style.size_pt === undefined ? undefined : `${style.size_pt}pt`,
    fontWeight: style.weight,
    fontStyle: style.italic ? "italic" : undefined,
    textDecoration: decorations || undefined,
    color: style.color,
    backgroundColor: style.highlight,
    letterSpacing: style.letter_spacing === undefined ? undefined : `${style.letter_spacing}pt`,
    fontVariantCaps: style.small_caps ? "small-caps" : undefined,
  };
}

/** Removes `undefined` entries so a patch can delete a field. Returns undefined when empty. */
export function compactStyle<T extends Record<string, unknown>>(style: T): T | undefined {
  const entries = Object.entries(style).filter(([, value]) => value !== undefined);
  return entries.length ? (Object.fromEntries(entries) as T) : undefined;
}

/** Merges a patch into a style, dropping every key the patch sets to `undefined`. */
export function patchStyle<T extends Record<string, unknown>>(base: T | undefined, patch: T): T {
  const merged: Record<string, unknown> = { ...base };
  for (const [key, value] of Object.entries(patch)) {
    if (value === undefined) delete merged[key];
    else merged[key] = value;
  }
  return merged as T;
}

/** Like `patchStyle`, but a font patch merges into the existing font and an empty family clears it. */
export function patchStyleWithFont<T extends { font?: FontRef } & Record<string, unknown>>(
  base: T | undefined,
  patch: T,
): T {
  const merged = patchStyle(base, patch);
  if ("font" in patch) {
    const font = patch.font ? { ...base?.font, ...patch.font } : undefined;
    if (font?.family) merged.font = font as T["font"];
    else delete merged.font;
  }
  return merged;
}
