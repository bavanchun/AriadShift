// The cloud jobs started in this browser. Whoever creates a job records its id
// here (the converter's cloud mode and the jobs page's Retry), and the jobs
// page reads each job's live state from the API. Anonymous jobs belong to no
// account, so this is the only list they have; a signed-in account's jobs come
// from `GET /v1/jobs` instead, and this record covers deployments without it.
// The record stays on this device: job ids of anonymous jobs are bearer
// secrets, and file names are never sent anywhere by this store.

export const JOB_HISTORY_KEY = "ariadshift-cloud-jobs";

/** Fired on `window` when this tab changes the history; other tabs get a `storage` event. */
export const JOB_HISTORY_EVENT = "ariadshift-cloud-jobs-changed";

/** Most entries kept; the oldest are dropped first. */
export const MAX_JOB_HISTORY = 200;

/** The API prunes expired job metadata after 30 days, so older entries can only 404. */
const PRUNE_AFTER_EXPIRY_MS = 30 * 24 * 60 * 60 * 1000;

export interface JobHistoryEntry {
  /** Job id. */
  id: string;
  /** Original input file name, for display only. */
  filename: string;
  /** User id of the account that created the job, or null for an anonymous job. */
  owner: string | null;
  createdAt: string;
  expiresAt: string;
}

type StorageLike = Pick<Storage, "getItem" | "setItem">;

function defaultStorage(): StorageLike | null {
  try {
    return typeof window === "undefined" ? null : window.localStorage;
  } catch {
    // Storage can be blocked (private modes, disabled site data).
    return null;
  }
}

function isEntry(value: unknown): value is JobHistoryEntry {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const entry = value as Record<string, unknown>;
  return (
    typeof entry.id === "string" &&
    typeof entry.filename === "string" &&
    (entry.owner === null || typeof entry.owner === "string") &&
    typeof entry.createdAt === "string" &&
    typeof entry.expiresAt === "string"
  );
}

function newestFirst(a: JobHistoryEntry, b: JobHistoryEntry): number {
  return Date.parse(b.createdAt) - Date.parse(a.createdAt);
}

/** Reads the history, newest first, without entries the API has certainly pruned. */
export function readJobHistory(
  storage: StorageLike | null = defaultStorage(),
  now = Date.now(),
): JobHistoryEntry[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(storage?.getItem(JOB_HISTORY_KEY) ?? "[]");
  } catch {
    return [];
  }
  if (!Array.isArray(parsed)) {
    return [];
  }
  return parsed
    .filter(isEntry)
    .filter((entry) => {
      const expires = Date.parse(entry.expiresAt);
      return Number.isNaN(expires) || now - expires < PRUNE_AFTER_EXPIRY_MS;
    })
    .sort(newestFirst);
}

function write(storage: StorageLike | null, entries: JobHistoryEntry[]): void {
  try {
    storage?.setItem(JOB_HISTORY_KEY, JSON.stringify(entries.slice(0, MAX_JOB_HISTORY)));
  } catch {
    // A full or blocked storage only loses the list, never a job.
    return;
  }
  if (typeof window !== "undefined") {
    window.dispatchEvent(new Event(JOB_HISTORY_EVENT));
  }
}

/** Records a job, replacing an older entry with the same id. */
export function rememberJob(entry: JobHistoryEntry, storage = defaultStorage()): void {
  const rest = readJobHistory(storage).filter((existing) => existing.id !== entry.id);
  write(storage, [entry, ...rest].sort(newestFirst));
}

/** Removes a job from this browser's list only; the job on the server is untouched. */
export function forgetJob(id: string, storage = defaultStorage()): void {
  write(
    storage,
    readJobHistory(storage).filter((entry) => entry.id !== id),
  );
}
