"use client";

import type { OutputFile } from "@ariadshift/sdk";
import { Button } from "@ariadshift/ui/components/button";
import {
  CircleCheckIcon,
  CircleXIcon,
  DownloadIcon,
  LoaderCircleIcon,
  RotateCcwIcon,
  Trash2Icon,
} from "lucide-react";
import { type ReactNode, type RefObject, useId, useState } from "react";
import type { CloudAction } from "@/lib/cloud/errors";
import {
  type CloudJobState,
  formatExpiry,
  isActive,
  isRunning,
  stepLabel,
} from "@/lib/cloud/steps";
import { formatBytes } from "@/lib/format-bytes";

export interface CloudJobRowProps {
  state: CloudJobState;
  fileName: string;
  host: string;
  now: number;
  statusRef: RefObject<HTMLParagraphElement | null>;
  errorRef: RefObject<HTMLHeadingElement | null>;
  tombstoneRef: RefObject<HTMLParagraphElement | null>;
  /** Seconds left before Retry may be used. */
  retryWait: number;
  /** Why Retry cannot run yet, for example a missing Turnstile token. */
  retryBlocked: string | null;
  /** Rendered above the error actions, for example a new Turnstile challenge. */
  retryExtra?: ReactNode;
  onCancel: () => void;
  onRetry: () => void;
  onDelete: () => void;
  onDownload: (output: OutputFile, deleteAfter: boolean) => void;
  onAction: (action: Exclude<CloudAction, "retry" | "delete">) => void;
  onConvertAgain: () => void;
}

const BAR = "h-2 w-full accent-primary";

const ACTION_LABELS: Record<Exclude<CloudAction, "retry" | "delete">, string> = {
  "sign-in": "Sign in",
  "use-anonymous": "Continue without an account",
  "use-device": "Convert on this device",
  turnstile: "Show the check again",
  "pick-target": "Pick another target",
};

function percent(state: CloudJobState): number {
  return state.total > 0 ? Math.floor((state.sent / state.total) * 100) : 0;
}

/** One file's cloud status: progress, result, expiry, deletion and errors. */
export function CloudJobRow(props: CloudJobRowProps) {
  const { state, fileName, host, now } = props;
  const [deleteAfter, setDeleteAfter] = useState(false);
  const deleteAfterHint = useId();
  const retryHint = useId();
  const label = stepLabel(state, now);
  const expiry = state.expiresAt ? formatExpiry(state.expiresAt, now) : null;
  const holdsFiles =
    state.upload !== null && state.phase !== "deleted" && state.phase !== "expired";
  const tombstone =
    state.phase === "deleted" || state.phase === "expired" || state.phase === "canceled";

  if (tombstone) {
    return (
      <div
        className="flex flex-col gap-3 rounded-lg border border-border p-4"
        data-testid="cloud-row"
      >
        <p
          ref={props.tombstoneRef}
          tabIndex={-1}
          className="text-sm outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
          data-testid="cloud-tombstone"
        >
          <strong className="font-semibold">{fileName}: </strong>
          {state.phase === "expired"
            ? (expiry ?? "Deleted automatically.")
            : (state.notice ??
              (state.phase === "canceled" ? "The conversion was canceled." : "Deleted."))}
        </p>
        <div>
          <Button variant="outline" size="lg" onClick={props.onConvertAgain}>
            <RotateCcwIcon aria-hidden="true" />
            Convert again
          </Button>
        </div>
      </div>
    );
  }

  const error = state.error;
  const failed = state.phase === "failed" || state.phase === "error";

  return (
    <div
      className="flex flex-col gap-4 rounded-lg border border-border p-4"
      data-testid="cloud-row"
    >
      <div className="flex flex-col gap-2">
        <p
          ref={props.statusRef}
          tabIndex={-1}
          className="flex items-center gap-2 text-sm font-semibold outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
          data-testid="cloud-status"
        >
          {failed ? (
            <CircleXIcon aria-hidden="true" className="size-4 text-destructive" />
          ) : state.phase === "succeeded" ? (
            <CircleCheckIcon aria-hidden="true" className="size-4" />
          ) : isActive(state.phase) ? (
            <LoaderCircleIcon aria-hidden="true" className="size-4 animate-spin" />
          ) : null}
          <span>
            {fileName} · {label}
          </span>
        </p>
        {state.phase === "uploading" && (
          <progress
            aria-label={`Uploading ${fileName}`}
            aria-valuetext={`${percent(state)} percent, ${formatBytes(state.sent)} of ${formatBytes(state.total)}`}
            max={100}
            value={percent(state)}
            className={BAR}
          />
        )}
        {(state.phase === "preparing" || state.phase === "checking" || isRunning(state.phase)) && (
          // No value: an indeterminate bar, since the server reports stages, not percentages.
          <progress aria-label={`${label.split(" · ")[0]} ${fileName}`} className={BAR} />
        )}
        {state.warnings.length > 0 && (
          <ul className="list-disc pl-5 text-sm text-muted-foreground" data-testid="cloud-warnings">
            {state.warnings.map((warning, index) => (
              <li key={`${warning.code}-${index}`}>{warning.message || warning.code}</li>
            ))}
          </ul>
        )}
        {expiry && holdsFiles && state.upload?.status === "completed" && (
          <p className="text-sm text-muted-foreground" data-testid="cloud-expiry">
            {expiry}
          </p>
        )}
      </div>

      {error && failed && (
        <div
          className="flex flex-col gap-2 rounded-lg border border-destructive/40 p-3"
          data-testid="cloud-error"
        >
          <h3
            ref={props.errorRef}
            tabIndex={-1}
            className="text-sm font-semibold text-destructive outline-none focus-visible:ring-3 focus-visible:ring-ring/50"
          >
            {error.title}
          </h3>
          <p className="text-sm">{error.message}</p>
          {error.serverCopy && <p className="text-sm">{error.serverCopy}</p>}
          {props.retryExtra}
          <div className="flex flex-wrap gap-2">
            {error.actions.includes("retry") && (
              <Button
                size="lg"
                aria-disabled={props.retryWait > 0 || props.retryBlocked !== null || undefined}
                aria-describedby={props.retryBlocked ? retryHint : undefined}
                onClick={() => {
                  if (props.retryWait <= 0 && props.retryBlocked === null) {
                    props.onRetry();
                  }
                }}
              >
                <RotateCcwIcon aria-hidden="true" />
                {props.retryWait > 0 ? `Retry in ${props.retryWait} s` : "Retry"}
              </Button>
            )}
            {error.actions.map((action) =>
              action === "retry" || action === "delete" ? null : (
                <Button
                  key={action}
                  size="lg"
                  variant="outline"
                  onClick={() => props.onAction(action)}
                >
                  {ACTION_LABELS[action]}
                </Button>
              ),
            )}
            {holdsFiles && (error.actions.includes("delete") || error.serverCopy) && (
              <Button
                size="lg"
                variant="destructive"
                aria-label={`Delete ${fileName} from the server now`}
                onClick={props.onDelete}
              >
                <Trash2Icon aria-hidden="true" />
                Delete now
              </Button>
            )}
          </div>
          {props.retryBlocked && (
            <p id={retryHint} className="text-sm text-muted-foreground">
              {props.retryBlocked}
            </p>
          )}
          {error.technical && (
            <details className="text-sm">
              <summary className="cursor-pointer">Technical details</summary>
              <p className="mt-1 break-words font-mono text-xs">{error.technical}</p>
            </details>
          )}
        </div>
      )}

      {state.phase === "succeeded" && state.outputs.length > 0 && (
        <div className="flex flex-col gap-3" data-testid="cloud-result">
          <ul className="flex flex-col gap-2">
            {state.outputs.map((output) => (
              <li key={output.id} className="flex flex-wrap items-center gap-3">
                <span className="text-sm">
                  <span data-testid="cloud-output-name">{output.filename}</span> ·{" "}
                  {formatBytes(output.size)}
                </span>
                <Button size="lg" onClick={() => props.onDownload(output, deleteAfter)}>
                  <DownloadIcon aria-hidden="true" />
                  Download {output.filename}
                </Button>
              </li>
            ))}
          </ul>
          <label className="flex min-h-11 items-start gap-2 text-sm">
            <input
              type="checkbox"
              checked={deleteAfter}
              onChange={(event) => setDeleteAfter(event.currentTarget.checked)}
              aria-describedby={deleteAfterHint}
              className="mt-0.5 size-4 accent-primary"
            />
            <span className="flex flex-col">
              <span className="font-medium">Delete after download</span>
              <span id={deleteAfterHint} className="text-muted-foreground">
                Deletes the files from {host} as soon as your download starts.
              </span>
            </span>
          </label>
        </div>
      )}

      <div className="flex flex-wrap gap-2">
        {(state.phase === "preparing" || state.phase === "uploading") && (
          <Button
            size="lg"
            variant="outline"
            onClick={props.onCancel}
            aria-label={`Cancel uploading ${fileName}`}
          >
            Cancel upload
          </Button>
        )}
        {isRunning(state.phase) && (
          <Button
            size="lg"
            variant="outline"
            onClick={props.onCancel}
            aria-label={`Cancel converting ${fileName} and delete it from the server`}
          >
            Cancel
          </Button>
        )}
        {state.phase === "succeeded" && holdsFiles && (
          <Button
            size="lg"
            variant="destructive"
            onClick={props.onDelete}
            aria-label={`Delete ${fileName} from the server now`}
          >
            <Trash2Icon aria-hidden="true" />
            Delete now
          </Button>
        )}
      </div>
    </div>
  );
}
