// Plain-language copy for cloud failures. The server's `code` picks the copy;
// its `message` appears only under "Technical details", so engine output and
// paths never become the main text.
import { AriadApiError } from "@ariadshift/sdk";
import { formatBytes } from "@/lib/format-bytes";
import { formatLabel } from "@/lib/formats";
import type { PlanLimits } from "./api";
import { AuthError } from "./auth";
import { formatClock } from "./steps";

/** What the user can do next; the panel renders one control per action. */
export type CloudAction =
  | "retry"
  | "sign-in"
  | "use-device"
  | "turnstile"
  | "pick-target"
  | "delete";

export interface CloudErrorView {
  code: string;
  title: string;
  message: string;
  /** Whether the file is on the server, and until when. */
  serverCopy: string | null;
  actions: CloudAction[];
  /** Seconds before Retry may be used, from `Retry-After`. */
  retryAfter: number | null;
  /** The server's own message and code, for "Technical details". */
  technical: string | null;
}

export interface CloudErrorContext {
  fileName: string;
  fileSize: number;
  from: string | null;
  to: string | null;
  limits: PlanLimits | null;
  /** Whether this deployment offers accounts. */
  signInOffered: boolean;
  signedIn: boolean;
  /** When the uploaded input is deleted, if it reached the server. */
  serverExpiresAt: string | null;
}

/** A job's final failure, from its `error` or its `failed` event. */
export interface JobFailure {
  code: string;
  message: string;
}

const SHORT_REASONS: Record<string, string> = {
  input_io: "the uploaded file could not be read",
  output_io: "the result could not be stored",
  storage: "the server's storage was unavailable",
  database: "the server's database was unavailable",
  tool_missing: "this server lacks an engine for this route",
  sandbox_unavailable: "the server could not isolate the engine",
  failed: "the engine stopped before it finished",
};

function planName(limits: PlanLimits | null): string {
  return limits ? `${limits.plan}` : "current";
}

function serverCopy(context: CloudErrorContext): string | null {
  return context.serverExpiresAt
    ? `The uploaded file is kept until ${formatClock(context.serverExpiresAt)} so you can retry, unless you delete it now.`
    : null;
}

function technical(code: string, message: string | undefined): string | null {
  return message ? `${message} (${code})` : code;
}

function withSignIn(context: CloudErrorContext, actions: CloudAction[]): CloudAction[] {
  return context.signInOffered && !context.signedIn ? ["sign-in", ...actions] : actions;
}

/** Copy for a request that failed before or while the file was sent or converted. */
export function describeCloudError(error: unknown, context: CloudErrorContext): CloudErrorView {
  const onServer = serverCopy(context);
  const base = { serverCopy: onServer, retryAfter: null as number | null };

  if (error instanceof AuthError) {
    return {
      ...base,
      code: error.code,
      title: "Sign-in service unavailable",
      message: error.message,
      serverCopy: null,
      actions: ["retry", "use-device"],
      technical: technical(error.code, error.message),
    };
  }

  if (!(error instanceof AriadApiError)) {
    const offline = typeof navigator !== "undefined" && navigator.onLine === false;
    return {
      ...base,
      code: "network",
      title: offline ? "You are offline" : "The server cannot be reached",
      message: context.serverExpiresAt
        ? `Your file is safe on the server until ${formatClock(context.serverExpiresAt)}.`
        : "Your file has not been uploaded.",
      serverCopy: null,
      actions: ["retry", "use-device"],
      technical: error instanceof Error ? error.message : String(error),
    };
  }

  const detail = technical(error.code, error.message);
  const max = context.limits?.limits.max_input_bytes;
  switch (error.code) {
    case "payload_too_large":
    case "limit_exceeded":
      if (error.status === 413) {
        return {
          ...base,
          code: error.code,
          title: "This file is too large for the cloud",
          message:
            typeof max === "number"
              ? `This file is ${formatBytes(context.fileSize)}. The ${planName(context.limits)} plan accepts files up to ${formatBytes(max)}.`
              : `This file is ${formatBytes(context.fileSize)}, more than the ${planName(context.limits)} plan accepts.`,
          actions: withSignIn(context, ["use-device"]),
          technical: detail,
        };
      }
      return {
        ...base,
        code: error.code,
        title: "This document needs a larger plan",
        message: `This document needs more than the ${planName(context.limits)} plan allows (${error.message}).`,
        actions: withSignIn(context, ["use-device"]),
        technical: detail,
      };
    case "quota_exceeded": {
      const jobs = context.limits?.quotas.jobs_per_day.limit;
      return {
        ...base,
        code: error.code,
        title: "Cloud allowance used up",
        message:
          typeof jobs === "number"
            ? `You have used today's ${jobs} cloud conversions. Each one frees up 24 hours after it started.`
            : "This plan's cloud allowance is used up for now. It frees up as earlier conversions pass 24 hours.",
        actions: withSignIn(context, ["use-device"]),
        technical: detail,
      };
    }
    case "rate_limited":
      return {
        ...base,
        code: error.code,
        title: "Too many requests",
        message: `Too many requests. Try again in ${error.retryAfter ?? 1} s.`,
        actions: ["retry"],
        retryAfter: error.retryAfter ?? 1,
        technical: detail,
      };
    case "turnstile_required":
    case "turnstile_failed":
      return {
        ...base,
        code: error.code,
        title: "Check failed",
        message: "We could not confirm you are not a bot. Complete the check again, then retry.",
        actions: ["retry"],
        technical: detail,
      };
    case "unsupported_route":
      return {
        ...base,
        code: error.code,
        title: "Not available on this server",
        message:
          context.from && context.to
            ? `${formatLabel(context.from)} to ${formatLabel(context.to)} is not available on this server.`
            : "This conversion is not available on this server.",
        actions: ["pick-target", "use-device"],
        technical: detail,
      };
    case "unauthorized":
      return {
        ...base,
        code: error.code,
        title: "Your session has ended",
        message: "Your session has ended. Sign in again; the file you chose stays here.",
        actions: context.signInOffered ? ["sign-in"] : [],
        technical: detail,
      };
    case "expired":
      return {
        ...base,
        code: error.code,
        title: "Already deleted",
        message: "This file was already deleted from the server.",
        serverCopy: null,
        actions: ["retry"],
        technical: detail,
      };
    case "conflict":
      return {
        ...base,
        code: error.code,
        title: "The upload did not finish",
        message: "The server did not receive the whole file.",
        actions: ["retry"],
        technical: detail,
      };
    case "invalid_request":
      return {
        ...base,
        code: error.code,
        title: "The upload did not match",
        message:
          "The file the server received does not match the one on this device, so the server deleted it.",
        serverCopy: null,
        actions: ["retry"],
        technical: detail,
      };
    case "transfer_failed":
      return {
        ...base,
        code: error.code,
        title: "The upload failed",
        message: "The storage server did not accept the file.",
        actions: ["retry", "use-device"],
        technical: detail,
      };
    case "download_failed":
      return {
        ...base,
        code: error.code,
        title: "The download failed",
        message: "The result could not be downloaded.",
        actions: ["retry"],
        technical: detail,
      };
    case "unavailable":
    case "internal":
      return {
        ...base,
        code: error.code,
        title: "The server cannot be reached",
        message: context.serverExpiresAt
          ? `Your file is safe on the server until ${formatClock(context.serverExpiresAt)}.`
          : "Your file has not been uploaded.",
        serverCopy: null,
        actions: ["retry", "use-device"],
        technical: detail,
      };
    default:
      return {
        ...base,
        code: error.code,
        title: "Something went wrong",
        message: "The server refused the request.",
        actions: ["retry", "use-device"],
        technical: detail,
      };
  }
}

/** Copy for a job that the server ran and that failed. */
export function describeJobFailure(
  failure: JobFailure,
  context: CloudErrorContext,
): CloudErrorView {
  if (failure.code === "limit_exceeded" || failure.code === "unsupported_route") {
    const view = describeCloudError(new AriadApiError(422, failure.code, failure.message), context);
    return { ...view, actions: [...view.actions, "delete"] };
  }
  const reason = SHORT_REASONS[failure.code] ?? "the engine reported an error";
  return {
    code: failure.code,
    title: "The conversion failed",
    message: `The conversion failed: ${reason}.`,
    serverCopy: serverCopy(context),
    actions: ["retry", "delete"],
    retryAfter: null,
    technical: technical(failure.code, failure.message),
  };
}
