export const COMPRESS_TOOLS = ["pdf.compress", "pdf.grayscale", "pdf.linearize"] as const;
export type CompressToolId = (typeof COMPRESS_TOOLS)[number];

export function isCompressTool(id: string): id is CompressToolId {
  return (COMPRESS_TOOLS as readonly string[]).includes(id);
}

export type CompressPresetId = "screen" | "ebook" | "print" | "lossless";

export interface CompressPreset {
  id: CompressPresetId;
  label: string;
  detail: string;
  /** Maximum image resolution; absent for the lossless profile. */
  dpi?: number;
  quality?: number;
}

export const COMPRESS_PRESETS: readonly CompressPreset[] = [
  { id: "screen", label: "Screen", detail: "72 DPI", dpi: 72, quality: 50 },
  { id: "ebook", label: "eBook", detail: "150 DPI", dpi: 150, quality: 70 },
  { id: "print", label: "Printer", detail: "300 DPI", dpi: 300, quality: 90 },
  { id: "lossless", label: "Prepress / Lossless", detail: "No image loss" },
];

export const DPI_MIN = 36;
export const DPI_MAX = 600;
export const QUALITY_MIN = 10;
export const QUALITY_MAX = 100;

export interface CompressSettings {
  preset: CompressPresetId | null;
  dpi: number | null;
  quality: number | null;
  grayscale: boolean;
}

export const DEFAULT_SETTINGS: CompressSettings = {
  preset: "ebook",
  dpi: 150,
  quality: 70,
  grayscale: false,
};

export interface SizeEstimate {
  /** Fraction of the original size the output is expected to keep. */
  ratio: number;
  low: number;
  high: number;
  /** Expected size reduction as a whole percentage, never negative. */
  reductionPercent: number;
}

const clamp = (value: number, min: number, max: number) => Math.min(max, Math.max(min, value));

/** Share of the original an output keeps, from image resolution and JPEG quality. */
export function compressionRatio(settings: CompressSettings): number {
  if (settings.dpi === null && settings.quality === null) return 0.92;
  const dpiFactor = Math.min(1, (settings.dpi ?? 300) / 300) ** 2;
  const qualityFactor = 0.3 + 0.7 * ((settings.quality ?? 90) / 100);
  const ratio = clamp(0.1 + 0.9 * dpiFactor * qualityFactor, 0.1, 1);
  return settings.grayscale ? ratio * 0.85 : ratio;
}

/** A range around the expected size: image-heavy files shrink more than text-heavy ones. */
export function estimateSize(originalBytes: number, ratio: number): SizeEstimate {
  const expected = originalBytes * ratio;
  return {
    ratio,
    low: Math.round(expected * 0.75),
    high: Math.min(originalBytes, Math.round(expected * 1.25)),
    reductionPercent: Math.max(0, Math.round((1 - ratio) * 100)),
  };
}

export function buildCompressParams(settings: CompressSettings): Record<string, unknown> {
  const params: Record<string, unknown> = {};
  if (settings.preset) params.preset = settings.preset;
  if (settings.dpi !== null) params.max_dpi = settings.dpi;
  if (settings.quality !== null) params.jpeg_quality = settings.quality;
  if (settings.grayscale) params.grayscale = true;
  return params;
}
