"use client";

import { Alert, AlertDescription, AlertTitle } from "@ariadshift/ui/components/alert";
import { Button } from "@ariadshift/ui/components/button";
import { Card, CardContent, CardHeader, CardTitle } from "@ariadshift/ui/components/card";
import {
  ArrowRightIcon,
  BookmarkPlusIcon,
  DownloadIcon,
  PencilIcon,
  Undo2Icon,
} from "lucide-react";
import { useState } from "react";
import { downloadOutput } from "@/converter/client";
import type { ToolExecutionArtifact } from "@/converter/protocol";
import { formatBytes } from "@/lib/format-bytes";
import { useWorkspaceCopy } from "./copy";
import { toolLabel } from "./tool-label";
import type { WorkspaceRun } from "./types";

export interface ToolResultCardProps {
  run: WorkspaceRun;
  locale?: string;
  /** Saves the output through the browser; defaults to a download link. */
  onDownload?: (artifact: ToolExecutionArtifact) => void | Promise<void>;
  /** Makes an output the workspace's input for the next tool. */
  onContinue: (artifact: ToolExecutionArtifact) => void | Promise<void>;
  /** Restores the input the tool received. */
  onUndo: () => void | Promise<void>;
  /** Offered only when the host can open the output in the editor. */
  onOpenInEditor?: (artifact: ToolExecutionArtifact) => void;
  /** Offered only when the host can store recipes. */
  onSaveRecipe?: () => void;
  className?: string;
}

function pageCountOf(run: WorkspaceRun): number | null {
  const { metrics, data } = run.result;
  for (const source of [metrics, data ?? {}]) {
    for (const key of ["page_count", "pages", "pages_out"]) {
      const value = (source as Record<string, unknown>)[key];
      if (typeof value === "number" && Number.isFinite(value)) return value;
    }
  }
  return null;
}

/** Before and after, measured, and four ways forward so a result is never a dead end. */
export function ToolResultCard({
  run,
  locale = "en",
  onDownload = (artifact) => downloadOutput(artifact.location, artifact.name),
  onContinue,
  onUndo,
  onOpenInEditor,
  onSaveRecipe,
  className,
}: ToolResultCardProps) {
  const copy = useWorkspaceCopy().result;
  const [failure, setFailure] = useState<string | null>(null);
  const { result } = run;
  const primary = result.artifacts[0];
  const outputSize = result.artifacts.reduce((sum, artifact) => sum + artifact.size, 0);
  const delta =
    run.input.size > 0 ? Math.round(((outputSize - run.input.size) / run.input.size) * 100) : 0;
  const pagesAfter = pageCountOf(run);
  const details = result.data && Object.keys(result.data).length > 0 ? result.data : null;

  async function guarded(action: () => void | Promise<void>) {
    setFailure(null);
    try {
      await action();
    } catch {
      setFailure(copy.downloadFailed);
    }
  }

  return (
    <Card data-slot="tool-result" className={className}>
      <CardHeader>
        <CardTitle>
          {copy.title}: {toolLabel(run.tool)}
        </CardTitle>
      </CardHeader>
      <CardContent className="flex flex-col gap-4">
        <table className="w-full text-sm">
          <thead>
            <tr className="text-start text-xs text-muted-foreground">
              <th scope="col" className="text-start font-medium" />
              <th scope="col" className="text-start font-medium">
                {copy.before}
              </th>
              <th scope="col" className="text-start font-medium">
                {copy.after}
              </th>
            </tr>
          </thead>
          <tbody className="tabular-nums">
            <tr>
              <th scope="row" className="text-start font-medium">
                {copy.size}
              </th>
              <td>{formatBytes(run.input.size, locale)}</td>
              <td>
                {formatBytes(outputSize, locale)}{" "}
                <span data-slot="result-delta" className="text-muted-foreground">
                  ({copy.change(delta)})
                </span>
              </td>
            </tr>
            <tr>
              <th scope="row" className="text-start font-medium">
                {copy.pages}
              </th>
              <td>{copy.pagesValue(run.input.pageCount)}</td>
              <td>{copy.pagesValue(pagesAfter)}</td>
            </tr>
            <tr>
              <th scope="row" className="text-start font-medium">
                {copy.duration}
              </th>
              <td />
              <td>{copy.seconds(result.elapsedMs)}</td>
            </tr>
          </tbody>
        </table>

        {result.warnings.length > 0 && (
          <Alert variant="default">
            <AlertTitle>{copy.warnings}</AlertTitle>
            <AlertDescription>
              <ul className="m-0 list-disc ps-4">
                {result.warnings.map((warning, index) => (
                  <li key={`${warning.code}-${index}`}>{warning.message}</li>
                ))}
              </ul>
            </AlertDescription>
          </Alert>
        )}
        {details && (
          <details className="text-sm">
            <summary className="cursor-pointer font-medium">{copy.details}</summary>
            <pre className="mt-2 max-h-60 overflow-auto rounded-lg border bg-muted p-2 text-xs">
              {JSON.stringify(details, null, 2)}
            </pre>
          </details>
        )}

        <ul aria-label={copy.outputs} className="m-0 flex list-none flex-col gap-2 p-0">
          {result.artifacts.map((artifact) => (
            <li key={artifact.name} className="flex flex-wrap items-center gap-2">
              <Button
                variant={artifact === primary ? "default" : "outline"}
                onClick={() => guarded(() => onDownload(artifact))}
              >
                <DownloadIcon aria-hidden="true" />
                {copy.download(artifact.name)}
              </Button>
              <span className="text-xs text-muted-foreground tabular-nums">
                {formatBytes(artifact.size, locale)}
              </span>
              <Button
                variant="secondary"
                size="sm"
                onClick={() => guarded(() => onContinue(artifact))}
              >
                <ArrowRightIcon aria-hidden="true" />
                {copy.continue}
              </Button>
              {onOpenInEditor && (
                <Button variant="ghost" size="sm" onClick={() => onOpenInEditor(artifact)}>
                  <PencilIcon aria-hidden="true" />
                  {copy.openInEditor}
                </Button>
              )}
            </li>
          ))}
        </ul>
        {failure && (
          <p role="alert" className="text-sm text-destructive">
            {failure}
          </p>
        )}
        <div className="flex flex-wrap gap-2">
          {onSaveRecipe && (
            <Button variant="outline" size="sm" onClick={onSaveRecipe}>
              <BookmarkPlusIcon aria-hidden="true" />
              {copy.saveRecipe}
            </Button>
          )}
          <Button variant="ghost" size="sm" onClick={() => guarded(onUndo)}>
            <Undo2Icon aria-hidden="true" />
            {copy.undo}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}
