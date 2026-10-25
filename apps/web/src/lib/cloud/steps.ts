// The user-facing steps of a cloud job. Server signals (upload progress, the
// job's SSE events, refusals) map onto one fixed set of steps, so a moment is
// always described by exactly one label, and only step changes are announced.
import type { Job, JobWarning, OutputFile, Upload } from "@ariadshift/sdk";
import type { CloudErrorView, JobFailure } from "./errors";

export type CloudPhase =
  /** The file is chosen; nothing has left the device. */
  | "staged"
  /** Hashing the file and reserving the upload. */
  | "preparing"
  | "uploading"
  /** The server verifies size and SHA-256 (`complete`). */
  | "checking"
  | "queued"
  | "converting"
  | "retrying"
  | "succeeded"
  | "failed"
  | "canceled"
  /** Deleted by the retention period. */
  | "expired"
  /** Deleted on request: Delete now, Delete after download, or a canceled job. */
  | "deleted"
  /** The transfer was canceled before a job existed. */
  | "upload-canceled"
  /** A request was refused or the server could not be reached. */
  | "error";

export interface CloudJobState {
  phase: CloudPhase;
  /** Bytes hashed on this device while preparing. */
  hashed: number;
  /** Bytes sent during the transfer, and the file's size. */
  sent: number;
  total: number;
  /** The last `progress` stage of the engine. */
  stage: string | null;
  attempt: number;
  /** When the current waiting step (queued, or converting and retrying) began, in ms. */
  since: number | null;
  /** The event stream dropped and the client is reconnecting. */
  reconnecting: boolean;
  warnings: JobWarning[];
  upload: Upload | null;
  job: Job | null;
  outputs: OutputFile[];
  /** Output ids the user downloaded. */
  downloaded: string[];
  error: CloudErrorView | null;
  failure: JobFailure | null;
  /** When the server deletes this job's files, or the upload's if no job exists. */
  expiresAt: string | null;
  /** The confirmation shown after a deletion or cancellation. */
  notice: string | null;
}

export const INITIAL_JOB_STATE: CloudJobState = {
  phase: "staged",
  hashed: 0,
  sent: 0,
  total: 0,
  stage: null,
  attempt: 0,
  since: null,
  reconnecting: false,
  warnings: [],
  upload: null,
  job: null,
  outputs: [],
  downloaded: [],
  error: null,
  failure: null,
  expiresAt: null,
  notice: null,
};

const ACTIVE: ReadonlySet<CloudPhase> = new Set([
  "preparing",
  "uploading",
  "checking",
  "queued",
  "converting",
  "retrying",
]);

/** Whether a request or job is under way. */
export function isActive(phase: CloudPhase): boolean {
  return ACTIVE.has(phase);
}

/** Whether the job runs on the server (queued, converting or retrying). */
export function isRunning(phase: CloudPhase): boolean {
  return phase === "queued" || phase === "converting" || phase === "retrying";
}

function record(value: unknown): Record<string, unknown> {
  return typeof value === "object" && value !== null ? (value as Record<string, unknown>) : {};
}

/** Applies one `JobEvent` (by kind and data); unknown kinds change nothing. */
export function applyJobEvent(
  state: CloudJobState,
  event: { kind: string; data?: unknown },
  now: number,
): CloudJobState {
  const data = record(event.data);
  const running = state.phase === "converting" || state.phase === "retrying";
  const base = { ...state, reconnecting: false };
  switch (event.kind) {
    case "queued":
      return { ...base, phase: "queued", since: state.phase === "queued" ? state.since : now };
    case "started":
      return {
        ...base,
        phase: "converting",
        attempt: typeof data.attempt === "number" ? data.attempt : state.attempt + 1,
        stage: null,
        since: running ? state.since : now,
      };
    case "progress":
      return {
        ...base,
        phase: "converting",
        stage: typeof data.stage === "string" ? data.stage : state.stage,
        since: running ? state.since : now,
      };
    case "retrying":
      return { ...base, phase: "retrying", since: running ? state.since : now };
    case "warning":
      return {
        ...base,
        warnings: [
          ...state.warnings,
          {
            code: typeof data.code === "string" ? data.code : "warning",
            message: typeof data.message === "string" ? data.message : "",
          },
        ],
      };
    case "succeeded":
      return { ...base, phase: "succeeded", since: null };
    case "failed":
      return {
        ...base,
        phase: "failed",
        since: null,
        failure: {
          code: typeof data.code === "string" ? data.code : "failed",
          message: typeof data.message === "string" ? data.message : "",
        },
      };
    case "canceled":
      return { ...base, phase: "canceled", since: null };
    case "expired":
      return { ...base, phase: data.reason === "deleted" ? "deleted" : "expired", since: null };
    default:
      return state;
  }
}

/** "0:16", "12:05" or "1:02:03". */
export function formatElapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = String(total % 60).padStart(2, "0");
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${seconds}`
    : `${minutes}:${seconds}`;
}

/** A local wall-clock time such as "19:45" or "7:45 PM". */
export function formatClock(iso: string): string {
  return new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(
    new Date(iso),
  );
}

function relative(ms: number): string {
  const minutes = Math.round(ms / 60_000);
  if (minutes < 1) {
    return "less than a minute";
  }
  if (minutes < 60) {
    return `${minutes} min`;
  }
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  return rest === 0 ? `${hours} h` : `${hours} h ${rest} min`;
}

/** "Deletes automatically at 19:45 (in 58 min)", or the past tense once it passed. */
export function formatExpiry(iso: string, now: number): string {
  const left = new Date(iso).getTime() - now;
  return left <= 0
    ? `Deleted automatically at ${formatClock(iso)}`
    : `Deletes automatically at ${formatClock(iso)} (in ${relative(left)})`;
}

/** A retention period in words: "1 hour", "30 minutes", "2 days". */
export function formatRetention(seconds: number): string {
  const plural = (value: number, unit: string) => `${value} ${unit}${value === 1 ? "" : "s"}`;
  if (seconds < 3600) {
    return plural(Math.max(1, Math.round(seconds / 60)), "minute");
  }
  if (seconds < 48 * 3600) {
    const hours = seconds / 3600;
    return Number.isInteger(hours)
      ? plural(hours, "hour")
      : plural(Math.round(seconds / 60), "minute");
  }
  return plural(Math.round(seconds / 86_400), "day");
}

/** The row's one label for the current step. */
export function stepLabel(state: CloudJobState, now: number): string {
  if (state.reconnecting && isRunning(state.phase)) {
    return "Reconnecting…";
  }
  const elapsed = state.since === null ? "" : ` · ${formatElapsed(now - state.since)}`;
  switch (state.phase) {
    case "staged":
      return "Not uploaded";
    case "preparing":
      return state.hashed > 0 && state.total > 0
        ? `Preparing ${Math.floor((state.hashed / state.total) * 100)}%`
        : "Preparing…";
    case "uploading":
      return `Uploading ${state.total > 0 ? Math.floor((state.sent / state.total) * 100) : 0}%`;
    case "checking":
      return "Checking file…";
    case "queued":
      return `Queued${elapsed}`;
    case "converting":
      return `Converting${state.stage ? `: ${state.stage}` : ""}${elapsed}`;
    case "retrying":
      return `Retrying (attempt ${state.attempt + 1})${elapsed}`;
    case "succeeded":
      return "Ready";
    case "failed":
      return "Failed";
    case "canceled":
      return "Canceled";
    case "expired":
      return "Deleted (expired)";
    case "deleted":
      return "Deleted";
    case "upload-canceled":
      return "Upload canceled";
    case "error":
      return state.error?.title ?? "Something went wrong";
  }
}

export interface Announcement {
  polite?: string;
  assertive?: string;
}

/** What to announce for a state change: step changes only, never each percent. */
export function announcementFor(
  previous: CloudJobState,
  next: CloudJobState,
  fileName: string,
): Announcement | null {
  if (next.phase === "error" && next.error && next.error !== previous.error) {
    return { assertive: `${next.error.title}. ${next.error.message}` };
  }
  if (next.phase === "failed" && previous.phase !== "failed") {
    return { assertive: `The conversion of ${fileName} failed.` };
  }
  if (next.reconnecting && !previous.reconnecting) {
    return { polite: "Reconnecting to the server." };
  }
  if (next.warnings.length > previous.warnings.length) {
    const count = next.warnings.length;
    return { polite: `${count} ${count === 1 ? "warning" : "warnings"}` };
  }
  if (next.phase === "succeeded" && next.outputs.length > 0 && previous.outputs.length === 0) {
    const names = next.outputs.map((output) => output.filename).join(", ");
    return { polite: `${names} ${next.outputs.length === 1 ? "is" : "are"} ready to download` };
  }
  if (next.notice && next.notice !== previous.notice) {
    return { polite: next.notice };
  }
  if (next.phase === previous.phase) {
    return next.phase === "converting" && next.stage !== previous.stage && next.stage
      ? { polite: `Converting: ${next.stage}` }
      : null;
  }
  switch (next.phase) {
    case "uploading":
      return { polite: `Uploading ${fileName}` };
    case "checking":
      return { polite: "Checking file" };
    case "queued":
      return { polite: "Queued" };
    case "converting":
      return { polite: next.stage ? `Converting: ${next.stage}` : "Converting" };
    case "retrying":
      return { polite: `Retrying (attempt ${next.attempt + 1})` };
    case "canceled":
      return { polite: "Canceled" };
    case "expired":
      return { polite: next.expiresAt ? formatExpiry(next.expiresAt, Date.now()) : "Deleted" };
    default:
      return null;
  }
}
