const UNITS = ["bytes", "KB", "MB", "GB"] as const;

/** Formats a byte count with binary multiples, for example "1.5 MB". */
export function formatBytes(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  if (unit === 0) {
    return `${value} ${value === 1 ? "byte" : "bytes"}`;
  }
  return `${value >= 10 ? Math.round(value) : value.toFixed(1)} ${UNITS[unit]}`;
}

/** Formats a 0–1 score as a percentage, for example "98%". */
export function formatScore(score: number | null): string {
  return score === null ? "not measured" : `${Math.round(score * 100)}%`;
}

/** Formats an estimated duration in milliseconds. */
export function formatDuration(ms: number | null): string {
  if (ms === null) {
    return "not measured";
  }
  if (ms < 1) {
    return "under 1 ms";
  }
  return ms < 1000 ? `${Math.round(ms)} ms` : `${(ms / 1000).toFixed(1)} s`;
}
