"use client";

import { Alert, AlertDescription, AlertTitle } from "@ariadshift/ui/components/alert";
import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { Progress } from "@ariadshift/ui/components/progress";
import { cn } from "@ariadshift/ui/lib/utils";
import {
  ArchiveIcon,
  CheckIcon,
  CircleAlertIcon,
  DownloadIcon,
  LoaderCircleIcon,
  MinusIcon,
  PlayIcon,
  RotateCcwIcon,
  Trash2Icon,
  XIcon,
} from "lucide-react";
import { useId } from "react";
import { formatBytes } from "@/lib/format-bytes";

export const BATCH_ITEM_STATUSES = ["queued", "running", "done", "failed", "cancelled"] as const;
export type BatchItemStatus = (typeof BATCH_ITEM_STATUSES)[number];

export interface BatchItem {
  id: string;
  name: string;
  status: BatchItemStatus;
  /** Size of the input file in bytes. */
  inputSize: number;
  /** Size of the produced file in bytes; set once the item is `done`. */
  outputSize?: number;
  /** Why the item failed, shown on its row. */
  error?: string;
}

/** The panel's strings. The shell passes the translated set; these are the English defaults. */
export interface BatchQueueCopy {
  title: string;
  empty: string;
  overall: (completed: number, total: number) => string;
  sizes: (input: string, output: string) => string;
  status: Record<BatchItemStatus, string>;
  runBatch: string;
  cancelBatch: string;
  retryFailed: string;
  downloadZip: string;
  clearCompleted: string;
  list: string;
  download: (name: string) => string;
  cancelItem: (name: string) => string;
  removeItem: (name: string) => string;
  guardTitle: string;
  guardBody: (limit: string, names: string) => string;
}

export const DEFAULT_BATCH_COPY: BatchQueueCopy = {
  title: "Batch queue",
  empty: "Add files to run this tool on all of them at once.",
  overall: (completed, total) => `${completed} of ${total} done`,
  sizes: (input, output) => `${input} in · ${output} out`,
  status: {
    queued: "Queued",
    running: "Running",
    done: "Done",
    failed: "Failed",
    cancelled: "Cancelled",
  },
  runBatch: "Run batch",
  cancelBatch: "Cancel batch",
  retryFailed: "Retry failed",
  downloadZip: "Download all (.zip)",
  clearCompleted: "Clear completed",
  list: "Files in the batch",
  download: (name) => `Download ${name}`,
  cancelItem: (name) => `Cancel ${name}`,
  removeItem: (name) => `Remove ${name}`,
  guardTitle: "Some files are too large for this browser",
  guardBody: (limit, names) =>
    `Files over ${limit} cannot be processed in the browser: ${names}. Remove them or run them with the ashift command on your computer.`,
};

export interface BatchQueuePanelProps {
  items: BatchItem[];
  /** Per-file browser budget in bytes; a larger file triggers the memory-guard banner. */
  fileBudgetBytes?: number;
  locale?: string;
  copy?: Partial<BatchQueueCopy>;
  onRunBatch: () => void;
  onCancelBatch: () => void;
  /** Re-runs only the `failed` and `cancelled` items. */
  onRetryFailed: () => void;
  onDownloadZip: () => void;
  onClearCompleted: () => void;
  /** Stops a `queued` or `running` item. */
  onCancelItem: (id: string) => void;
  /** Drops an item that is not running. */
  onRemoveItem: (id: string) => void;
  onDownloadItem: (id: string) => void;
  className?: string;
}

const STATUS_ICON: Record<BatchItemStatus, typeof CheckIcon> = {
  queued: MinusIcon,
  running: LoaderCircleIcon,
  done: CheckIcon,
  failed: CircleAlertIcon,
  cancelled: XIcon,
};

/** A multi-file queue for any single-input tool: progress, sizes, per-file state and actions. */
export function BatchQueuePanel({
  items,
  fileBudgetBytes,
  locale = "en",
  copy: copyOverrides,
  onRunBatch,
  onCancelBatch,
  onRetryFailed,
  onDownloadZip,
  onClearCompleted,
  onCancelItem,
  onRemoveItem,
  onDownloadItem,
  className,
}: BatchQueuePanelProps) {
  const copy = { ...DEFAULT_BATCH_COPY, ...copyOverrides };
  const titleId = useId();
  const count = (status: BatchItemStatus) => items.filter((item) => item.status === status).length;
  const total = items.length;
  const completed = count("done");
  const active = count("running") > 0;
  const hasQueued = count("queued") > 0;
  const hasRetryable = count("failed") + count("cancelled") > 0;
  const inputBytes = items.reduce((sum, item) => sum + item.inputSize, 0);
  const outputBytes = items.reduce((sum, item) => sum + (item.outputSize ?? 0), 0);
  const oversized =
    fileBudgetBytes === undefined ? [] : items.filter((item) => item.inputSize > fileBudgetBytes);

  return (
    <section
      data-slot="batch-queue"
      aria-labelledby={titleId}
      className={cn("grid gap-3 rounded-xl border bg-card p-4 text-card-foreground", className)}
    >
      <h2 id={titleId} className="text-sm font-semibold">
        {copy.title}
      </h2>

      {total === 0 ? (
        <p data-slot="batch-empty" className="text-sm text-muted-foreground">
          {copy.empty}
        </p>
      ) : (
        <>
          <div className="grid gap-1.5">
            <Progress
              variant="golden"
              value={total === 0 ? 0 : (completed / total) * 100}
              aria-label={copy.overall(completed, total)}
            />
            <div className="flex flex-wrap justify-between gap-x-4 text-xs text-muted-foreground">
              <span data-slot="batch-overall">{copy.overall(completed, total)}</span>
              <span data-slot="batch-sizes">
                {copy.sizes(formatBytes(inputBytes, locale), formatBytes(outputBytes, locale))}
              </span>
            </div>
          </div>

          {oversized.length > 0 && fileBudgetBytes !== undefined && (
            <Alert variant="destructive" data-slot="batch-memory-guard">
              <CircleAlertIcon aria-hidden="true" />
              <AlertTitle>{copy.guardTitle}</AlertTitle>
              <AlertDescription>
                {copy.guardBody(
                  formatBytes(fileBudgetBytes, locale),
                  oversized.map((item) => item.name).join(", "),
                )}
              </AlertDescription>
            </Alert>
          )}

          <div className="flex flex-wrap gap-2" data-slot="batch-actions">
            <Button size="sm" disabled={!hasQueued || active} onClick={onRunBatch}>
              <PlayIcon aria-hidden="true" />
              {copy.runBatch}
            </Button>
            <Button
              size="sm"
              variant="outline"
              disabled={!active && !hasQueued}
              onClick={onCancelBatch}
            >
              <XIcon aria-hidden="true" />
              {copy.cancelBatch}
            </Button>
            <Button
              size="sm"
              variant="outline"
              disabled={!hasRetryable || active}
              onClick={onRetryFailed}
            >
              <RotateCcwIcon aria-hidden="true" />
              {copy.retryFailed}
            </Button>
            <Button size="sm" variant="outline" disabled={completed === 0} onClick={onDownloadZip}>
              <ArchiveIcon aria-hidden="true" />
              {copy.downloadZip}
            </Button>
            <Button size="sm" variant="ghost" disabled={completed === 0} onClick={onClearCompleted}>
              <Trash2Icon aria-hidden="true" />
              {copy.clearCompleted}
            </Button>
          </div>

          <ul aria-label={copy.list} className="m-0 grid list-none gap-1.5 p-0">
            {items.map((item) => {
              const Icon = STATUS_ICON[item.status];
              const cancellable = item.status === "queued" || item.status === "running";
              return (
                <li
                  key={item.id}
                  data-status={item.status}
                  className="grid gap-1 rounded-lg border px-3 py-2"
                >
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="min-w-0 flex-1 truncate text-sm font-medium">{item.name}</span>
                    <Badge variant={item.status === "failed" ? "destructive" : "secondary"}>
                      <Icon
                        aria-hidden="true"
                        className={cn(
                          item.status === "running" && "animate-spin motion-reduce:animate-none",
                        )}
                      />
                      {copy.status[item.status]}
                    </Badge>
                    <span className="text-xs text-muted-foreground">
                      {item.outputSize === undefined
                        ? formatBytes(item.inputSize, locale)
                        : copy.sizes(
                            formatBytes(item.inputSize, locale),
                            formatBytes(item.outputSize, locale),
                          )}
                    </span>
                    {item.status === "done" && (
                      <Button
                        size="icon-sm"
                        variant="ghost"
                        aria-label={copy.download(item.name)}
                        onClick={() => onDownloadItem(item.id)}
                      >
                        <DownloadIcon aria-hidden="true" />
                      </Button>
                    )}
                    {cancellable ? (
                      <Button
                        size="icon-sm"
                        variant="ghost"
                        aria-label={copy.cancelItem(item.name)}
                        onClick={() => onCancelItem(item.id)}
                      >
                        <XIcon aria-hidden="true" />
                      </Button>
                    ) : (
                      <Button
                        size="icon-sm"
                        variant="ghost"
                        aria-label={copy.removeItem(item.name)}
                        onClick={() => onRemoveItem(item.id)}
                      >
                        <Trash2Icon aria-hidden="true" />
                      </Button>
                    )}
                  </div>
                  {item.status === "failed" && item.error && (
                    <p className="text-xs text-destructive">{item.error}</p>
                  )}
                </li>
              );
            })}
          </ul>
        </>
      )}
    </section>
  );
}
