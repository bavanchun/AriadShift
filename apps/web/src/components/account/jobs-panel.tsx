"use client";

// The signed-in account's cloud jobs, listed by the API (`GET /v1/jobs`), so
// a job started anywhere shows up here. Deployments without that endpoint, and
// anonymous jobs (which belong to no account), fall back to the jobs recorded
// in this browser. Each row reads its live state from `/v1`.

import type { Job } from "@ariadshift/sdk";
import { Alert, AlertDescription, AlertTitle } from "@ariadshift/ui/components/alert";
import { Button } from "@ariadshift/ui/components/button";
import { useCallback, useEffect, useId, useMemo, useState } from "react";
import { useAccount } from "./account-context";
import { displayHost } from "./cloud-config";
import {
  forgetJob,
  JOB_HISTORY_EVENT,
  JOB_HISTORY_KEY,
  type JobHistoryEntry,
  readJobHistory,
  rememberJob,
} from "./job-history";
import { JobRow } from "./job-row";
import { apiErrorCopy } from "./job-status";
import { createJobsApi, type JobsApi, type ListedJob } from "./jobs-api";
import { signInHref } from "./session-gate";

export interface JobsPanelProps {
  /** Replaces the SDK client; receives whether the requests carry the user's token. */
  createApi?: (signedIn: boolean) => JobsApi;
  pollIntervalMs?: number;
}

/** The account's job list as the API returned it. */
type AccountList =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "unsupported" }
  | { kind: "error"; message: string }
  | {
      kind: "ready";
      jobs: ListedJob[];
      nextCursor: string | null;
      more: { kind: "idle" } | { kind: "loading" } | { kind: "error"; message: string };
    };

interface Row {
  entry: JobHistoryEntry;
  initialJob?: Job;
}

/** Keeps the list in step with this tab's changes and with other tabs. */
function useJobHistory(owner: string | null | undefined): JobHistoryEntry[] {
  const [entries, setEntries] = useState<JobHistoryEntry[]>([]);
  useEffect(() => {
    if (owner === undefined) {
      return;
    }
    const refresh = () => setEntries(readJobHistory().filter((entry) => entry.owner === owner));
    const onStorage = (event: StorageEvent) => {
      if (event.key === null || event.key === JOB_HISTORY_KEY) {
        refresh();
      }
    };
    refresh();
    window.addEventListener(JOB_HISTORY_EVENT, refresh);
    window.addEventListener("storage", onStorage);
    return () => {
      window.removeEventListener(JOB_HISTORY_EVENT, refresh);
      window.removeEventListener("storage", onStorage);
    };
  }, [owner]);
  return entries;
}

/** Reads the account's jobs from the API when signed in, one page at a time. */
function useAccountJobs(api: JobsApi, signedIn: boolean) {
  const [list, setList] = useState<AccountList>({ kind: "idle" });
  const [attempt, setAttempt] = useState(0);

  useEffect(() => {
    if (!signedIn) {
      setList({ kind: "idle" });
      return;
    }
    // Read so a retry re-runs this effect.
    void attempt;
    const controller = new AbortController();
    setList({ kind: "loading" });
    api
      .listJobs({ signal: controller.signal })
      .then((page) => {
        if (controller.signal.aborted) {
          return;
        }
        setList(
          page.supported
            ? {
                kind: "ready",
                jobs: page.jobs,
                nextCursor: page.nextCursor,
                more: { kind: "idle" },
              }
            : { kind: "unsupported" },
        );
      })
      .catch((error: unknown) => {
        if (!controller.signal.aborted) {
          setList({ kind: "error", message: apiErrorCopy(error) });
        }
      });
    return () => controller.abort();
  }, [api, signedIn, attempt]);

  const reload = useCallback(() => setAttempt((count) => count + 1), []);

  const loadMore = useCallback(async () => {
    if (list.kind !== "ready" || list.nextCursor === null || list.more.kind === "loading") {
      return;
    }
    const cursor = list.nextCursor;
    setList({ ...list, more: { kind: "loading" } });
    try {
      const page = await api.listJobs({ cursor });
      setList((current) => {
        if (current.kind !== "ready") {
          return current;
        }
        if (!page.supported) {
          return { ...current, nextCursor: null, more: { kind: "idle" } };
        }
        const known = new Set(current.jobs.map((job) => job.id));
        return {
          kind: "ready",
          jobs: [...current.jobs, ...page.jobs.filter((job) => !known.has(job.id))],
          nextCursor: page.nextCursor,
          more: { kind: "idle" },
        };
      });
    } catch (error) {
      setList((current) =>
        current.kind === "ready"
          ? { ...current, more: { kind: "error", message: apiErrorCopy(error) } }
          : current,
      );
    }
  }, [api, list]);

  /** Shows a job this page just created first, without reading the list again. */
  const prepend = useCallback((job: ListedJob) => {
    setList((current) =>
      current.kind === "ready"
        ? { ...current, jobs: [job, ...current.jobs.filter((known) => known.id !== job.id)] }
        : current,
    );
  }, []);

  return { list, reload, loadMore, prepend };
}

export function JobsPanel({ createApi, pollIntervalMs }: JobsPanelProps) {
  const { config, auth, session, refreshSession, announce } = useAccount();
  const headingId = useId();
  const [focusId, setFocusId] = useState<string | null>(null);
  const host = displayHost(config.apiUrl);

  const signedIn = session.status === "signed-in";
  // undefined until the session is known, so no request goes out with the wrong credentials.
  const owner =
    session.status === "signed-in"
      ? session.user.id
      : session.status === "signed-out"
        ? null
        : undefined;
  const entries = useJobHistory(owner);

  const api = useMemo(
    () => (createApi ?? ((user: boolean) => createJobsApi(config.apiUrl, auth, user)))(signedIn),
    [createApi, config.apiUrl, auth, signedIn],
  );
  const { list, reload, loadMore, prepend } = useAccountJobs(api, signedIn);
  const fromServer = list.kind === "ready";
  // Signed in, the browser's list is shown only once the API turns out not to list jobs.
  const listPending = signedIn && (list.kind === "idle" || list.kind === "loading");

  const rows = useMemo<Row[] | null>(() => {
    if (owner === undefined) {
      return null;
    }
    if (list.kind === "ready") {
      const names = new Map(entries.map((entry) => [entry.id, entry.filename]));
      return list.jobs.map((job) => ({
        entry: {
          id: job.id,
          filename: job.filename ?? names.get(job.id) ?? `Job ${job.id.slice(0, 8)}`,
          owner,
          createdAt: job.created_at,
          expiresAt: job.expires_at,
        },
        initialJob: job,
      }));
    }
    if (listPending) {
      return null;
    }
    return entries.map((entry) => ({ entry }));
  }, [owner, list, listPending, entries]);

  const onRetried = useCallback(
    (entry: JobHistoryEntry, job: Job) => {
      rememberJob(entry);
      prepend({ ...job, filename: entry.filename });
      setFocusId(entry.id);
    },
    [prepend],
  );

  const onForget = useCallback(
    (id: string) => {
      const entry = entries.find((candidate) => candidate.id === id);
      forgetJob(id);
      announce(
        `${entry?.filename ?? "The job"} was removed from this list. Its files on the server are unchanged.`,
      );
      document.getElementById(headingId)?.focus();
    },
    [entries, announce, headingId],
  );

  const scope = fromServer
    ? `Cloud conversions of your account on ${host}, from any device.`
    : signedIn
      ? "Cloud conversions started in this browser with your account. This list is kept on this device only."
      : "Cloud conversions started in this browser without an account. This list is kept on this device only.";

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col gap-2">
        <h1 className="text-3xl font-bold tracking-tight">Jobs</h1>
        <p className="text-muted-foreground">
          {scope} Files stay on {host} until they are deleted automatically, or until you delete them here.
        </p>
      </div>

      {session.status === "loading" ? (
        <p role="status" className="text-sm text-muted-foreground">
          Checking your session…
        </p>
      ) : null}

      {session.status === "error" ? (
        <Alert variant="destructive">
          <AlertTitle>Cannot check your session</AlertTitle>
          <AlertDescription>{session.message}</AlertDescription>
          <div className="pt-2">
            <Button variant="outline" onClick={() => void refreshSession()}>
              Try again
            </Button>
          </div>
        </Alert>
      ) : null}

      {session.status === "signed-out" ? (
        <p className="text-sm">
          Showing jobs without an account.{" "}
          <a href={signInHref("/jobs")} className="text-primary underline-offset-4 hover:underline">
            Sign in
          </a>{" "}
          to see the jobs of your account.
        </p>
      ) : null}

      {listPending ? (
        <p role="status" className="text-sm text-muted-foreground">
          Loading your jobs…
        </p>
      ) : null}

      {signedIn && list.kind === "unsupported" ? (
        <p className="text-sm text-muted-foreground">
          {host} cannot list your account's jobs yet, so this page shows the jobs started in this
          browser.
        </p>
      ) : null}

      {signedIn && list.kind === "error" ? (
        <Alert variant="destructive">
          <AlertTitle>Cannot load your jobs from {host}</AlertTitle>
          <AlertDescription>
            {list.message} Meanwhile, this page shows the jobs started in this browser.
          </AlertDescription>
          <div className="pt-2">
            <Button variant="outline" onClick={reload}>
              Try again
            </Button>
          </div>
        </Alert>
      ) : null}

      {rows ? (
        <section aria-labelledby={headingId} className="flex flex-col gap-3">
          <h2 id={headingId} tabIndex={-1} className="text-xl font-semibold outline-none">
            {rows.length === 1 ? "1 job" : `${rows.length} jobs`}
          </h2>
          {rows.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              No cloud jobs yet. Start one from the{" "}
              <a href="/app" className="text-primary underline-offset-4 hover:underline">
                converter
              </a>
              .
            </p>
          ) : (
            <ul className="flex flex-col gap-3">
              {rows.map(({ entry, initialJob }) => (
                <JobRow
                  key={entry.id}
                  entry={entry}
                  initialJob={initialJob}
                  api={api}
                  serverHost={host}
                  focusOnLoad={entry.id === focusId}
                  onRetried={onRetried}
                  // The server's list would bring a removed row back.
                  onForget={fromServer ? undefined : onForget}
                  pollIntervalMs={pollIntervalMs}
                />
              ))}
            </ul>
          )}
          {list.kind === "ready" && list.nextCursor !== null ? (
            <div className="flex flex-col items-start gap-2">
              <Button
                variant="outline"
                disabled={list.more.kind === "loading"}
                onClick={() => void loadMore()}
              >
                {list.more.kind === "loading" ? "Loading more jobs…" : "Show more jobs"}
              </Button>
              {list.more.kind === "error" ? (
                <p role="alert" className="text-sm text-destructive">
                  {list.more.message}
                </p>
              ) : null}
            </div>
          ) : null}
        </section>
      ) : null}
    </div>
  );
}
