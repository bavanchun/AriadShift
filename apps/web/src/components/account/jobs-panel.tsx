"use client";

// The cloud jobs started in this browser, for the signed-in account or, when
// signed out, the anonymous jobs. Each row reads its live state from `/v1`.

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
import { createJobsApi, type JobsApi } from "./jobs-api";
import { signInHref } from "./session-gate";

export interface JobsPanelProps {
  /** Replaces the SDK client; receives whether the requests carry the user's token. */
  createApi?: (signedIn: boolean) => JobsApi;
  pollIntervalMs?: number;
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

  const onRetried = useCallback((entry: JobHistoryEntry) => {
    rememberJob(entry);
    setFocusId(entry.id);
  }, []);

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

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-col gap-2">
        <h1 className="text-3xl font-bold tracking-tight">Jobs</h1>
        <p className="text-muted-foreground">
          Cloud conversions started in this browser
          {signedIn ? " with your account" : " without an account"}. This list is kept on this
          device only. Files stay on {host} until they are deleted automatically, or until you
          delete them here.
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

      {owner !== undefined ? (
        <section aria-labelledby={headingId} className="flex flex-col gap-3">
          <h2 id={headingId} tabIndex={-1} className="text-xl font-semibold outline-none">
            {entries.length === 1 ? "1 job" : `${entries.length} jobs`}
          </h2>
          {entries.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              No cloud jobs yet. Start one from the{" "}
              <a href="/app" className="text-primary underline-offset-4 hover:underline">
                converter
              </a>
              .
            </p>
          ) : (
            <ul className="flex flex-col gap-3">
              {entries.map((entry) => (
                <JobRow
                  key={entry.id}
                  entry={entry}
                  api={api}
                  serverHost={host}
                  focusOnLoad={entry.id === focusId}
                  onRetried={onRetried}
                  onForget={onForget}
                  pollIntervalMs={pollIntervalMs}
                />
              ))}
            </ul>
          )}
        </section>
      ) : null}
    </div>
  );
}
