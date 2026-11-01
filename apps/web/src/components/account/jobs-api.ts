// The `/v1` calls the jobs page makes, through the SDK. A signed-in user's
// requests carry a JWT from the auth service; anonymous jobs are read without
// credentials, since a bearer token would look for them in the wrong workspace.

import {
  AriadApiError,
  type AriadClient,
  createAriadClient,
  errorFromBody,
  type Job,
} from "@ariadshift/sdk";
import type { AuthApi } from "./auth-api";

/** Jobs requested per page of the account's job list. */
export const JOB_PAGE_SIZE = 50;

/** A job from the account's list; the API adds the input's original file name. */
export interface ListedJob extends Job {
  filename?: string;
}

/**
 * One page of `GET /v1/jobs`, or `supported: false` when the deployment does
 * not serve that endpoint yet, so the page can fall back to this browser's list.
 */
export type JobListPage =
  | { supported: false }
  | { supported: true; jobs: ListedJob[]; nextCursor: string | null };

export interface ListJobsOptions {
  cursor?: string;
  limit?: number;
  signal?: AbortSignal;
}

export type JobsApi = Pick<
  AriadClient,
  "getJob" | "events" | "outputs" | "download" | "deleteJob" | "createJob"
> & {
  /** Lists the signed-in workspace's jobs, newest first. */
  listJobs(options?: ListJobsOptions): Promise<JobListPage>;
};

/** Statuses meaning the API has no `GET /v1/jobs` route (an older deployment). */
const UNSUPPORTED_LIST_STATUSES = new Set([404, 405, 501]);

function isListedJob(value: unknown): value is ListedJob {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const job = value as Record<string, unknown>;
  return (
    typeof job.id === "string" &&
    typeof job.status === "string" &&
    typeof job.created_at === "string" &&
    typeof job.expires_at === "string" &&
    (job.filename === undefined || typeof job.filename === "string")
  );
}

/** Reads one page of `GET /v1/jobs` through the SDK's generated client. */
export async function listJobs(
  client: Pick<AriadClient, "raw">,
  { cursor, limit = JOB_PAGE_SIZE, signal }: ListJobsOptions = {},
): Promise<JobListPage> {
  const { data, error, response } = await client.raw.get({
    url: "/v1/jobs",
    query: cursor === undefined ? { limit } : { limit, cursor },
    signal,
  });
  if (response === undefined) {
    // The request never got a response (network failure, CORS refusal or abort).
    throw error instanceof Error ? error : new Error(String(error));
  }
  if (UNSUPPORTED_LIST_STATUSES.has(response.status)) {
    return { supported: false };
  }
  if (!response.ok || error !== undefined) {
    throw errorFromBody(response, error);
  }
  const body = data as { jobs?: unknown; next_cursor?: unknown } | undefined;
  if (!Array.isArray(body?.jobs) || !body.jobs.every(isListedJob)) {
    throw new Error("The server answered the job list with an unexpected body.");
  }
  return {
    supported: true,
    jobs: body.jobs,
    nextCursor: typeof body.next_cursor === "string" ? body.next_cursor : null,
  };
}

async function requireBearerToken(auth: AuthApi): Promise<string> {
  let token: string | undefined;
  try {
    token = await auth.apiToken();
  } catch (error) {
    const status =
      typeof error === "object" &&
      error !== null &&
      "status" in error &&
      typeof (error as { status?: unknown }).status === "number"
        ? (error as { status: number }).status
        : 401;
    throw new AriadApiError(status, "unauthorized", "Your session has ended. Sign in again.");
  }
  if (typeof token !== "string" || token === "") {
    throw new AriadApiError(401, "unauthorized", "Your session has ended. Sign in again.");
  }
  return token;
}

export function createJobsApi(apiUrl: string, auth: AuthApi, signedIn: boolean): JobsApi {
  const client = createAriadClient({
    baseUrl: apiUrl,
    ...(signedIn ? { credentials: { bearer: () => requireBearerToken(auth) } } : {}),
  });
  return { ...client, listJobs: (options) => listJobs(client, options) };
}

/** Saves a downloaded file through a temporary object URL. */
export function saveBlob(blob: Blob, filename: string): void {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  link.rel = "noopener";
  document.body.append(link);
  link.click();
  link.remove();
  // Revoke after the browser has started reading the URL.
  setTimeout(() => URL.revokeObjectURL(url), 60_000);
}
