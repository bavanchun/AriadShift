const UNITS = ["KB", "MB", "GB"] as const;

/**
 * Formats a byte count with binary multiples, for example "1.5 MB". Numbers and the word
 * "bytes" follow `locale`; the KB, MB and GB symbols are shared by every locale.
 */
export function formatBytes(bytes: number, locale = "en"): string {
  if (bytes < 1024) {
    return new Intl.NumberFormat(locale, {
      style: "unit",
      unit: "byte",
      unitDisplay: "long",
    }).format(bytes);
  }
  let value = bytes;
  let unit = -1;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = value >= 10 ? 0 : 1;
  const number = new Intl.NumberFormat(locale, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(value);
  return `${number} ${UNITS[unit]}`;
}

/** Formats a 0–1 score as a percentage, for example "98%". `missing` is shown for null. */
export function formatScore(score: number | null, locale = "en", missing = "not measured"): string {
  return score === null
    ? missing
    : new Intl.NumberFormat(locale, { style: "percent", maximumFractionDigits: 0 }).format(score);
}

/** Formats an estimated duration in milliseconds; `missing` and `underOneMs` are translated. */
export function formatDuration(
  ms: number | null,
  locale = "en",
  text: { missing: string; underOneMs: string } = {
    missing: "not measured",
    underOneMs: "under 1 ms",
  },
): string {
  if (ms === null) {
    return text.missing;
  }
  if (ms < 1) {
    return text.underOneMs;
  }
  const format = (value: number, unit: "ms" | "s", digits: number) =>
    `${new Intl.NumberFormat(locale, {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    }).format(value)} ${unit}`;
  return ms < 1000 ? format(Math.round(ms), "ms", 0) : format(ms / 1000, "s", 1);
}
