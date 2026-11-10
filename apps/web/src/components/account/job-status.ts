// Plain-language status, expiry and error copy for cloud jobs, translated with the
// `web.account` translator the caller passes in. The step labels
// follow the converter's cloud mode, so a job reads the same in /app and /jobs.

import type { Job, JobStatus } from "@ariadshift/sdk";
import type { Translate } from "./account-ui";

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
export function jobStepLabel(job: Job, live: LiveJobState, t: Translate, now = Date.now()): string {
  const reconnecting = (step: string) =>
    live.reconnecting ? t("jobs.status.reconnecting", { step }) : step;
  switch (job.status) {
    case "queued":
      return reconnecting(t("jobs.status.queued"));
    case "running":
      return reconnecting(
        live.retryAttempt !== undefined
          ? t("jobs.status.retrying", { attempt: live.retryAttempt })
          : live.stage
            ? t("jobs.status.converting_stage", { stage: live.stage })
            : t("jobs.status.converting"),
      );
    case "succeeded":
      return t("jobs.status.ready");
    case "failed":
      return t("jobs.status.failed");
    case "canceled":
      return t("jobs.status.canceled");
    case "expired":
      // DELETE expires a job before its retention time; retention expires it at that time.
      return t(
        Date.parse(job.expires_at) <= now ? "jobs.status.deleted_expired" : "jobs.status.deleted",
      );
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
export function formatRelativeUntil(ms: number, t: Translate): string {
  const minutes = Math.floor(ms / 60_000);
  if (minutes < 1) {
    return t("jobs.until.less_than_minute");
  }
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours === 0) {
    return t("jobs.until.minutes", { minutes });
  }
  return rest === 0
    ? t("jobs.until.hours", { hours })
    : t("jobs.until.hours_minutes", { hours, minutes: rest });
}

/** Local clock time, with the date when it is not today. */
export function formatClock(iso: string, locale: string, now = Date.now()): string {
  const date = new Date(iso);
  const sameDay = date.toDateString() === new Date(now).toDateString();
  return sameDay
    ? date.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" })
    : date.toLocaleString(locale, { dateStyle: "medium", timeStyle: "short" });
}

/** When the job's files go away, as an absolute and a relative time. */
export function expiryLine(job: Job, t: Translate, locale: string, now = Date.now()): string {
  const expires = Date.parse(job.expires_at);
  const clock = formatClock(job.expires_at, locale, now);
  if (job.status === "expired") {
    return expires <= now
      ? t("jobs.expiry.deleted_at", { clock })
      : t("jobs.expiry.deleted_from_server");
  }
  if (expires <= now) {
    return t("jobs.expiry.being_deleted");
  }
  return t("jobs.expiry.deletes_at", { clock, relative: formatRelativeUntil(expires - now, t) });
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
export function jobErrorCopy(code: string | undefined, t: Translate): string {
  switch (code) {
    case "limit_exceeded":
    case "unsupported_route":
    case "input_io":
    case "tool_missing":
      return t(`jobs.failure.${code}`);
    case "output_io":
    case "storage":
    case "database":
      return t("jobs.failure.storage");
    default:
      return t("jobs.failure.other");
  }
}

/** The fields of an `AriadApiError` this page uses; network failures have none. */
interface ApiFailure {
  status?: number;
  code?: string;
  retryAfter?: number;
}

/** Copy for a refused or failed API request. */
export function apiErrorCopy(error: unknown, t: Translate): string {
  const failure = (typeof error === "object" && error !== null ? error : {}) as ApiFailure;
  switch (failure.code) {
    case "unauthorized":
      return t("errors.session_ended");
    case "not_found":
      return t("errors.job_gone");
    case "expired":
      return t("errors.file_deleted");
    case "conflict":
      return t("errors.job_conflict");
    case "rate_limited":
      return failure.retryAfter
        ? t("errors.rate_limited_after", { seconds: failure.retryAfter })
        : t("errors.rate_limited");
    case "quota_exceeded":
      return t("errors.quota_exceeded");
    case "unavailable":
      return t("errors.sign_in_unverifiable");
    default:
      if (failure.status !== undefined) {
        return t("errors.request_failed");
      }
      // fetch rejects with a TypeError when the network fails; other errors are local checks.
      return error instanceof TypeError
        ? t("errors.network_unreachable")
        : t("errors.transfer_incomplete");
  }
}
