"use client";
// Runs the open single-input tool on every file with `BatchRunner` and shows the queue. The
// runner owns its own converter worker, so a batch never competes with the workspace's tool run.
import { useCallback, useEffect, useMemo, useState } from "react";
import { BatchQueuePanel } from "@/components/batch";
import {
  type BatchProgress,
  BatchRunner,
  DEFAULT_BATCH_FILE_BUDGET_BYTES,
} from "@/converter/batch";
import { downloadOutput } from "@/converter/client";
import type { ToolDescriptor } from "@/converter/tool-catalog";
import { useWorkspaceCopy } from "./copy";
import type { WorkspaceFile } from "./types";

export interface BatchSectionProps {
  tool: ToolDescriptor;
  /** The parameters the inspector currently holds, applied to every file. */
  params: Record<string, unknown>;
  paramsValid: boolean;
  files: WorkspaceFile[];
  locale?: string;
  /** Largest single input the browser accepts. */
  fileBudgetBytes?: number;
  onRemoveFile: (id: string) => void | Promise<void>;
  /** Tests supply a runner over a stub client. */
  createRunner?: () => BatchRunner;
  className?: string;
}

const message = (error: unknown) =>
  typeof error === "object" && error !== null && "message" in error
    ? String((error as { message: unknown }).message)
    : String(error);

export function BatchSection({
  tool,
  params,
  paramsValid,
  files,
  locale,
  fileBudgetBytes = DEFAULT_BATCH_FILE_BUDGET_BYTES,
  onRemoveFile,
  createRunner = () => new BatchRunner(),
  className,
}: BatchSectionProps) {
  const copy = useWorkspaceCopy().batch;
  const [runner] = useState(createRunner);
  const [progress, setProgress] = useState<BatchProgress | null>(null);
  const [dismissed, setDismissed] = useState<ReadonlySet<string>>(new Set());
  const [problem, setProblem] = useState<string | null>(null);

  useEffect(() => {
    const stop = runner.onProgress(setProgress);
    return () => {
      stop();
      runner.dispose();
    };
  }, [runner]);

  // A different tool or file set makes earlier results stale.
  const fileKeys = files.map((file) => file.key).join("\n");
  useEffect(() => {
    runner.cancelBatch();
    setProgress(null);
    setDismissed(new Set());
    setProblem(null);
  }, [runner, tool.id, fileKeys]);

  const items = useMemo(
    () =>
      progress
        ? progress.items
            .filter((item) => !dismissed.has(item.id))
            .map((item) => ({
              id: item.id,
              name: item.file.name,
              status: item.status,
              inputSize: item.file.size,
              ...(item.result
                ? {
                    outputSize: item.result.artifacts.reduce((sum, entry) => sum + entry.size, 0),
                  }
                : {}),
              ...(item.error ? { error: item.error.message } : {}),
            }))
        : files.map((file) => ({
            id: file.id,
            name: file.name,
            status: "queued" as const,
            inputSize: file.size,
          })),
    [progress, dismissed, files],
  );

  const guarded = useCallback(async (action: () => Promise<unknown>) => {
    setProblem(null);
    try {
      await action();
    } catch (error) {
      setProblem(message(error));
    }
  }, []);

  const run = () => {
    if (!paramsValid) {
      setProblem(copy.invalid);
      return;
    }
    void guarded(() =>
      progress
        ? runner.retryFailed()
        : runner.run(
            tool.id,
            params,
            files.map((file) => file.file),
          ),
    );
  };

  const downloadItem = (id: string) => {
    const item = progress?.items.find((entry) => entry.id === id);
    const artifact = item?.result?.artifacts[0];
    if (artifact) void guarded(() => downloadOutput(artifact.location, artifact.name));
  };

  const downloadZip = () =>
    guarded(async () => {
      const archive = await runner.packageBatchZip(`${tool.id.replace(/\W+/g, "-")}-batch.zip`);
      await downloadOutput({ kind: "memory", blob: archive }, archive.name);
    });

  return (
    <div className="grid gap-2" data-slot="batch-section">
      <BatchQueuePanel
        className={className}
        items={items}
        fileBudgetBytes={fileBudgetBytes}
        locale={locale}
        copy={copy}
        onRunBatch={run}
        onCancelBatch={() => runner.cancelBatch()}
        onRetryFailed={() => void guarded(() => runner.retryFailed())}
        onDownloadZip={() => void downloadZip()}
        onClearCompleted={() =>
          setDismissed(
            (previous) =>
              new Set([
                ...previous,
                ...(progress?.items.filter((item) => item.status === "done") ?? []).map(
                  (item) => item.id,
                ),
              ]),
          )
        }
        // Before a run an item is a workspace file: dropping it from the queue removes the file.
        onCancelItem={(id) => (progress ? runner.cancelItem(id) : void onRemoveFile(id))}
        onRemoveItem={(id) =>
          progress ? setDismissed((previous) => new Set([...previous, id])) : void onRemoveFile(id)
        }
        onDownloadItem={downloadItem}
      />
      {problem && (
        <p role="alert" className="text-sm text-destructive">
          {copy.failed(problem)}
        </p>
      )}
    </div>
  );
}
