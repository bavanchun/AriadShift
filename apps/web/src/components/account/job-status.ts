// Plain-language status, expiry and error copy for cloud jobs. The step labels
// follow the converter's cloud mode, so a job reads the same in /app and /jobs.

import type { Job, JobStatus } from "@ariadshift/sdk";

/** What the event stream has said about a running job since its last fetch. */
export interface LiveJobState {
  /** Stage from the latest `progress` event, for example `write`. */
  stage?: string;
  /** Set after a `retrying` event: the attempt that is about to run. */
  retryAttempt?: number;
  /** The event stream dropped and the page is polling instead. */
  reconnecting?: boolean;
}

export function isActive(status: JobStatus): boolean {
  return status === "queued" || status === "running";
}

/** The one label that names the job's current step. */
export function jobStepLabel(job: Job, live: LiveJobState = {}, now = Date.now()): string {
  switch (job.status) {
    case "queued":
      return live.reconnecting ? "Queued · Reconnecting…" : "Queued";
    case "running": {
      const step =
        live.retryAttempt !== undefined
          ? `Retrying (attempt ${live.retryAttempt})`
          : live.stage
            ? `Converting: ${live.stage}`
            : "Converting";
      return live.reconnecting ? `${step} · Reconnecting…` : step;
    }
    case "succeeded":
      return "Ready";
    case "failed":
      return "Failed";
    case "canceled":
      return "Canceled";
    case "expired":
      // DELETE expires a job before its retention time; retention expires it at that time.
      return Date.parse(job.expires_at) <= now ? "Deleted (expired)" : "Deleted";
  }
}

/** `0:16`, `12:05` or `1:02:03`. */
export function formatElapsed(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = String(total % 60).padStart(2, "0");
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, "0")}:${seconds}`
    : `${minutes}:${seconds}`;
}

/** `in 58 min`, `in 2 h 5 min` or `in less than a minute`. */
export function formatRelativeUntil(ms: number): string {
  const minutes = Math.floor(ms / 60_000);
  if (minutes < 1) {
    return "in less than a minute";
  }
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours === 0) {
    return `in ${minutes} min`;
  }
  return rest === 0 ? `in ${hours} h` : `in ${hours} h ${rest} min`;
}

/** Local clock time, with the date when it is not today. */
export function formatClock(iso: string, now = Date.now()): string {
  const date = new Date(iso);
  const sameDay = date.toDateString() === new Date(now).toDateString();
  return sameDay
    ? date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

/** When the job's files go away, as an absolute and a relative time. */
export function expiryLine(job: Job, now = Date.now()): string {
  const expires = Date.parse(job.expires_at);
  const clock = formatClock(job.expires_at, now);
  if (job.status === "expired") {
    return expires <= now
      ? `Deleted automatically at ${clock}.`
      : "Deleted from the server. The input and output files are gone.";
  }
  if (expires <= now) {
    return "Being deleted now.";
  }
  return `Deletes automatically at ${clock} (${formatRelativeUntil(expires - now)}).`;
}

/** Job error codes caused by the document or its input: retrying cannot help. */
const FINAL_ERROR_CODES: ReadonlySet<string> = new Set([
  "limit_exceeded",
  "unsupported_route",
  "input_io",
]);

/** A failed or canceled job can run again while its input upload is still kept. */
export function canRetry(job: Job, now = Date.now()): boolean {
  if (job.status !== "failed" && job.status !== "canceled") {
    return false;
  }
  if (Date.parse(job.expires_at) <= now) {
    return false;
  }
  return !(job.error && FINAL_ERROR_CODES.has(job.error.code));
}

/** Copy for a failed job's `error.code`; the server message goes under "Technical details". */
export function jobErrorCopy(code: string | undefined): string {
  switch (code) {
    case "limit_exceeded":
      return "This document needs more than your plan allows. Try a smaller file, or convert it on this device.";
    case "unsupported_route":
      return "This server cannot convert between these formats. Pick another target in the converter.";
    case "input_io":
      return "The uploaded file could not be read or is no longer on the server. Upload it again.";
    case "output_io":
    case "storage":
    case "database":
      return "The server could not store the result. Try again in a moment.";
    case "tool_missing":
      return "This server is missing the engine the route needs. Ask its operator, or pick another target.";
    default:
      return "The conversion failed. Try again, or convert the file on this device.";
  }
}

/** The fields of an `AriadApiError` this page uses; network failures have none. */
interface ApiFailure {
  status?: number;
  code?: string;
  retryAfter?: number;
}

/** Copy for a refused or failed API request. */
export function apiErrorCopy(error: unknown): string {
  const failure = (typeof error === "object" && error !== null ? error : {}) as ApiFailure;
  switch (failure.code) {
    case "unauthorized":
      return "Your session has ended. Sign in again.";
    case "not_found":
      return "This job is no longer on the server.";
    case "expired":
      return "This file was already deleted.";
    case "conflict":
      return "The job is not ready, or its uploaded file is no longer on the server.";
    case "rate_limited":
      return failure.retryAfter
        ? `Too many requests. Try again in ${failure.retryAfter} s.`
        : "Too many requests. Try again shortly.";
    case "quota_exceeded":
      return "You have used your plan's cloud conversions for now. Convert on this device, or try later.";
    case "unavailable":
      return "The server cannot verify your sign-in right now. Try again shortly.";
    default:
      if (failure.status !== undefined) {
        return "The server could not complete the request. Try again shortly.";
      }
      // fetch rejects with a TypeError when the network fails; other errors are local checks.
      return error instanceof TypeError
        ? "The server cannot be reached. Check your connection and try again."
        : "The transfer did not complete. Try again.";
  }
}
