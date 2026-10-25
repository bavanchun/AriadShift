"use client";

// One cloud job: its live step (followed over SSE, with polling when the
// stream drops), expiry, outputs, and the retry, download, delete and
// remove-from-list actions.

import type { Job, JobEvent, OutputFile } from "@ariadshift/sdk";
import { Alert, AlertDescription } from "@ariadshift/ui/components/alert";
import { Button } from "@ariadshift/ui/components/button";
import {
  CircleCheckIcon,
  CircleXIcon,
  DownloadIcon,
  LoaderCircleIcon,
  RotateCcwIcon,
  Trash2Icon,
} from "lucide-react";
import { type ReactNode, useCallback, useEffect, useId, useRef, useState } from "react";
import { formatBytes } from "@/lib/format-bytes";
import { formatLabel } from "@/lib/formats";
import { useAccount } from "./account-context";
import type { JobHistoryEntry } from "./job-history";
import {
  apiErrorCopy,
  canRetry,
  expiryLine,
  formatElapsed,
  isActive,
  jobErrorCopy,
  jobStepLabel,
  type LiveJobState,
} from "./job-status";
import { type JobsApi, saveBlob } from "./jobs-api";
import { useNow } from "./use-now";

/** How often to read the job when its event stream is unavailable. */
export const POLL_INTERVAL_MS = 5_000;

type RowState =
  | { kind: "loading" }
  | { kind: "error"; message: string }
  | { kind: "gone" }
  | { kind: "job"; job: Job };

function errorCode(error: unknown): string | undefined {
  return typeof error === "object" && error !== null && "code" in error
    ? String((error as { code: unknown }).code)
    : undefined;
}

export interface JobRowProps {
  entry: JobHistoryEntry;
  /** The job as the account's job list returned it, so the row needs no first read. */
  initialJob?: Job;
  api: JobsApi;
  /** Host name of the API, for the deletion confirmation. */
  serverHost: string;
  /** Focus the row's status once it has loaded (after a retry created it). */
  focusOnLoad?: boolean;
  onRetried: (entry: JobHistoryEntry, job: Job) => void;
  /** Removes the row from this browser's list; omitted for rows from the server's list. */
  onForget?: (id: string) => void;
  pollIntervalMs?: number;
}

export function JobRow({
  entry,
  initialJob,
  api,
  serverHost,
  focusOnLoad = false,
  onRetried,
  onForget,
  pollIntervalMs = POLL_INTERVAL_MS,
}: JobRowProps) {
  const { announce } = useAccount();
  const titleId = useId();
  const [state, setState] = useState<RowState>(
    initialJob ? { kind: "job", job: initialJob } : { kind: "loading" },
  );
  const [live, setLive] = useState<LiveJobState>({});
  const [outputs, setOutputs] = useState<OutputFile[] | null>(null);
  const [deleted, setDeleted] = useState(false);
  const [busy, setBusy] = useState<"download" | "delete" | "retry" | null>(null);
  const [rowError, setRowError] = useState<string | null>(null);
  const statusRef = useRef<HTMLParagraphElement>(null);
  const deletedRef = useRef<HTMLParagraphElement>(null);

  const job = state.kind === "job" ? state.job : null;
  const status = job?.status ?? null;
  const active = status !== null && isActive(status);
  const now = useNow(active ? 1_000 : 30_000);
  // Read by the event handler when a retry starts, without restarting the stream.
  const attempts = useRef(0);
  attempts.current = job?.attempts ?? 0;

  const load = useCallback(
    async (signal?: AbortSignal) => {
      try {
        const fresh = await api.getJob(entry.id, signal);
        setState({ kind: "job", job: fresh });
      } catch (error) {
        if (signal?.aborted) {
          return;
        }
        setState(
          errorCode(error) === "not_found"
            ? { kind: "gone" }
            : { kind: "error", message: apiErrorCopy(error) },
        );
      }
    },
    [api, entry.id],
  );

  // A job handed over by the list is current; read it only when it was not.
  const hasInitialJob = useRef(initialJob !== undefined);
  useEffect(() => {
    if (hasInitialJob.current) {
      hasInitialJob.current = false;
      return;
    }
    const controller = new AbortController();
    void load(controller.signal);
    return () => controller.abort();
  }, [load]);

  // Follow a queued or running job over SSE; poll when the stream fails.
  useEffect(() => {
    if (!active) {
      return;
    }
    const controller = new AbortController();
    let poll: ReturnType<typeof setInterval> | undefined;

    const apply = (event: JobEvent) => {
      switch (event.kind) {
        case "started":
          setLive({});
          setState((current) =>
            current.kind === "job"
              ? { kind: "job", job: { ...current.job, status: "running" } }
              : current,
          );
          break;
        case "progress":
          setLive({ stage: String(event.data.stage ?? "") || undefined });
          break;
        case "retrying":
          setLive((current) => ({
            retryAttempt: (current.retryAttempt ?? attempts.current) + 1,
          }));
          break;
      }
    };

    void (async () => {
      try {
        for await (const event of api.events(entry.id, { signal: controller.signal })) {
          apply(event);
        }
        setLive({});
        await load(controller.signal);
      } catch {
        if (controller.signal.aborted) {
          return;
        }
        setLive((current) => ({ ...current, reconnecting: true }));
        poll = setInterval(() => void load(controller.signal), pollIntervalMs);
      }
    })();

    return () => {
      controller.abort();
      if (poll) {
        clearInterval(poll);
      }
    };
  }, [active, api, entry.id, load, pollIntervalMs]);

  // Announce transitions seen while the page is open, not the state found on load.
  const previousStatus = useRef(status);
  useEffect(() => {
    const previous = previousStatus.current;
    previousStatus.current = status;
    if (previous === null || previous === status || deleted) {
      return;
    }
    if (status === "succeeded") {
      announce(`${entry.filename} is converted and ready to download.`);
    } else if (status === "failed") {
      announce(
        `Conversion of ${entry.filename} failed. ${jobErrorCopy(job?.error?.code)}`,
        "assertive",
      );
    } else if (status === "canceled") {
      announce(`The conversion of ${entry.filename} was canceled.`);
    }
  }, [status, deleted, announce, entry.filename, job?.error?.code]);

  // Show the output names and sizes before download.
  useEffect(() => {
    if (status !== "succeeded") {
      return;
    }
    const controller = new AbortController();
    api
      .outputs(entry.id, controller.signal)
      .then((result) => setOutputs(result.outputs))
      .catch((error: unknown) => {
        if (!controller.signal.aborted) {
          setRowError(apiErrorCopy(error));
        }
      });
    return () => controller.abort();
  }, [status, api, entry.id]);

  const loaded = state.kind !== "loading";
  useEffect(() => {
    if (focusOnLoad && loaded) {
      statusRef.current?.focus();
    }
  }, [focusOnLoad, loaded]);

  useEffect(() => {
    if (deleted) {
      deletedRef.current?.focus();
    }
  }, [deleted]);

  const onDownload = async (output: OutputFile) => {
    setBusy("download");
    setRowError(null);
    try {
      // Presigned URLs last five minutes, so ask for fresh ones on every download.
      const fresh = (await api.outputs(entry.id)).outputs;
      const target = fresh.find((candidate) => candidate.id === output.id) ?? output;
      saveBlob(await api.download(target), target.filename);
      announce(`Downloading ${target.filename}.`);
    } catch (error) {
      setRowError(apiErrorCopy(error));
      if (errorCode(error) === "expired") {
        void load();
      }
    } finally {
      setBusy(null);
    }
  };

  const onDelete = async () => {
    setBusy("delete");
    setRowError(null);
    try {
      await api.deleteJob(entry.id);
      setState((current) =>
        current.kind === "job"
          ? { kind: "job", job: { ...current.job, status: "expired" } }
          : current,
      );
      setOutputs(null);
      setDeleted(true);
      announce(
        `Deleted ${entry.filename} from ${serverHost}. The input and output files are gone.`,
      );
    } catch (error) {
      if (errorCode(error) === "not_found") {
        setState({ kind: "gone" });
      } else {
        setRowError(apiErrorCopy(error));
      }
    } finally {
      setBusy(null);
    }
  };

  const onRetry = async () => {
    if (!job) {
      return;
    }
    setBusy("retry");
    setRowError(null);
    try {
      const next = await api.createJob({
        uploadId: job.upload_id,
        to: job.to,
        profile: job.profile,
      });
      announce(`Retrying ${entry.filename}.`);
      onRetried(
        {
          id: next.id,
          filename: entry.filename,
          owner: entry.owner,
          createdAt: next.created_at,
          expiresAt: next.expires_at,
        },
        next,
      );
    } catch (error) {
      setRowError(
        errorCode(error) === "conflict"
          ? "The uploaded file is no longer on the server. Upload it again from the converter."
          : apiErrorCopy(error),
      );
    } finally {
      setBusy(null);
    }
  };

  const removeButton = onForget ? (
    <Button variant="ghost" size="sm" onClick={() => onForget(entry.id)}>
      Remove from this list<span className="sr-only"> ({entry.filename})</span>
    </Button>
  ) : null;

  let body: ReactNode;
  if (state.kind === "loading") {
    body = (
      <p ref={statusRef} tabIndex={-1} className="text-sm text-muted-foreground">
        Loading status…
      </p>
    );
  } else if (state.kind === "gone") {
    body = (
      <>
        <p ref={statusRef} tabIndex={-1} className="text-sm">
          This job is no longer on the server.
        </p>
        <div className="flex flex-wrap gap-2">{removeButton}</div>
      </>
    );
  } else if (state.kind === "error") {
    body = (
      <>
        <p ref={statusRef} tabIndex={-1} className="text-sm text-destructive">
          {state.message}
        </p>
        <div className="flex flex-wrap gap-2">
          <Button variant="outline" size="sm" onClick={() => void load()}>
            Try again<span className="sr-only"> to load {entry.filename}</span>
          </Button>
          {removeButton}
        </div>
      </>
    );
  } else {
    const current = state.job;
    const label = jobStepLabel(current, live, now);
    const started = Date.parse(current.started_at ?? current.created_at);
    const Icon = active
      ? LoaderCircleIcon
      : current.status === "succeeded"
        ? CircleCheckIcon
        : current.status === "failed"
          ? CircleXIcon
          : current.status === "expired"
            ? Trash2Icon
            : null;
    body = (
      <>
        <p ref={statusRef} tabIndex={-1} className="flex items-center gap-2 text-sm">
          {Icon ? (
            <Icon
              aria-hidden="true"
              className={
                active
                  ? "size-4 animate-spin"
                  : current.status === "failed"
                    ? "size-4 text-destructive"
                    : "size-4"
              }
            />
          ) : null}
          <span className="font-medium">{label}</span>
          {active ? (
            <span className="text-muted-foreground tabular-nums">
              · {formatElapsed(now - started)}
            </span>
          ) : null}
        </p>

        {current.status === "failed" ? (
          <div className="flex flex-col gap-1 text-sm">
            <p>{jobErrorCopy(current.error?.code)}</p>
            {current.error ? (
              <details>
                <summary className="cursor-pointer text-muted-foreground">
                  Technical details
                </summary>
                <p className="mt-1 font-mono text-xs break-all">
                  {current.error.code}: {current.error.message}
                </p>
              </details>
            ) : null}
          </div>
        ) : null}

        {current.warnings.length > 0 && current.status !== "expired" ? (
          <details className="text-sm">
            <summary className="cursor-pointer">
              {current.warnings.length === 1 ? "1 warning" : `${current.warnings.length} warnings`}
            </summary>
            <ul className="mt-1 list-disc pl-5 text-muted-foreground">
              {current.warnings.map((warning, index) => (
                // Warnings have no id and may repeat; the list never reorders.
                <li key={`${index}-${warning.code}`}>{warning.message}</li>
              ))}
            </ul>
          </details>
        ) : null}

        {deleted ? (
          <p ref={deletedRef} tabIndex={-1} className="text-sm">
            Deleted from {serverHost}. The input and output files are gone.
          </p>
        ) : (
          <p className="text-xs text-muted-foreground">{expiryLine(current, now)}</p>
        )}

        {outputs && outputs.length > 0 ? (
          <ul className="flex flex-col gap-2">
            {outputs.map((output) => (
              <li key={output.id} className="flex flex-wrap items-center gap-2 text-sm">
                <Button size="sm" disabled={busy !== null} onClick={() => void onDownload(output)}>
                  <DownloadIcon aria-hidden="true" />
                  Download {output.filename}
                </Button>
                <span className="text-muted-foreground">{formatBytes(output.size)}</span>
              </li>
            ))}
          </ul>
        ) : null}

        <div className="flex flex-wrap gap-2">
          {canRetry(current, now) && !deleted ? (
            <Button
              variant="outline"
              size="sm"
              disabled={busy !== null}
              onClick={() => void onRetry()}
            >
              <RotateCcwIcon aria-hidden="true" />
              Retry<span className="sr-only"> {entry.filename}</span>
            </Button>
          ) : null}
          {current.status !== "expired" ? (
            <Button
              variant="destructive"
              size="sm"
              disabled={busy !== null}
              onClick={() => void onDelete()}
            >
              <Trash2Icon aria-hidden="true" />
              Delete now<span className="sr-only"> {entry.filename} from the server</span>
            </Button>
          ) : null}
          {removeButton}
        </div>
      </>
    );
  }

  return (
    <li
      aria-labelledby={titleId}
      className="flex flex-col gap-2 rounded-xl p-4 ring-1 ring-foreground/10"
    >
      <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
        <h3 id={titleId} className="font-medium break-all">
          {entry.filename}
        </h3>
        {job ? (
          <span className="text-xs text-muted-foreground">
            {formatLabel(job.from)} → {formatLabel(job.to)}
          </span>
        ) : null}
      </div>
      {body}
      {rowError ? (
        <Alert variant="destructive">
          <AlertDescription>{rowError}</AlertDescription>
        </Alert>
      ) : null}
    </li>
  );
}
