"use client";

import { Badge } from "@ariadshift/ui/components/badge";
import { Button } from "@ariadshift/ui/components/button";
import { ShieldAlertIcon } from "lucide-react";
import { useEffect, useId, useState } from "react";
import { type DocumentModeLabels, fillLabel } from "./document-mode-labels";
import type { VersionHistory } from "./use-version-history";

export interface VersionHistoryDrawerProps {
  history: VersionHistory;
  labels: DocumentModeLabels;
  locale?: string;
  className?: string;
}

/** Saved versions of the open document, with restore, manual checkpoints and the redaction purge. */
export function VersionHistoryDrawer({
  history,
  labels,
  locale,
  className,
}: VersionHistoryDrawerProps) {
  const headingId = useId();
  const [label, setLabel] = useState("");
  const [restored, setRestored] = useState(false);
  const [busy, setBusy] = useState(false);
  const { refresh } = history;
  // Other tabs and earlier sessions may have saved versions since the editor opened.
  useEffect(() => {
    void refresh();
  }, [refresh]);

  const saveCheckpoint = async () => {
    setBusy(true);
    try {
      if (await history.saveCheckpoint(label)) setLabel("");
    } finally {
      setBusy(false);
    }
  };

  return (
    <aside
      aria-labelledby={headingId}
      className={`flex w-80 shrink-0 flex-col gap-3 overflow-y-auto border-s border-border bg-card p-3 ${className ?? ""}`}
    >
      <h2 id={headingId} className="text-sm font-semibold">
        {labels.historyTitle}
      </h2>

      {history.purgedCount !== null && (
        <div
          role="status"
          data-testid="redaction-purge-banner"
          className="flex gap-2 rounded-md border border-border bg-muted p-2 text-sm"
        >
          <ShieldAlertIcon aria-hidden="true" className="mt-0.5 size-4 shrink-0" />
          <div className="flex flex-col gap-1">
            <p>{fillLabel(labels.redactionPurged, { count: history.purgedCount })}</p>
            <p className="text-xs text-muted-foreground">{labels.redactionDisabledAutosave}</p>
          </div>
        </div>
      )}

      {history.error && (
        <p role="alert" className="text-sm text-destructive">
          {fillLabel(labels.historyError, { reason: history.error })}
        </p>
      )}

      <form
        className="flex items-end gap-2"
        onSubmit={(event) => {
          event.preventDefault();
          void saveCheckpoint();
        }}
      >
        <label className="flex min-w-0 flex-1 flex-col gap-0.5 text-xs text-muted-foreground">
          {labels.checkpointLabel}
          <input
            type="text"
            value={label}
            disabled={history.locked}
            className="h-7 w-full rounded-md border border-border bg-background px-2 text-sm"
            onChange={(event) => setLabel(event.target.value)}
          />
        </label>
        <Button type="submit" size="sm" disabled={history.locked || busy}>
          {labels.saveManualCheckpoint}
        </Button>
      </form>

      {restored && (
        <p role="status" className="text-sm text-muted-foreground">
          {labels.versionRestored}
        </p>
      )}

      {history.versions.length === 0 ? (
        <p className="text-sm text-muted-foreground">{labels.historyEmpty}</p>
      ) : (
        <ul aria-label={labels.historyTitle} className="flex flex-col gap-2">
          {history.versions.map((version) => {
            const when = new Date(version.createdAt).toLocaleString(locale);
            return (
              <li
                key={version.versionId}
                data-version-id={version.versionId}
                data-kind={version.kind}
                className="flex flex-col gap-1 rounded-md border border-border p-2 text-sm"
              >
                <div className="flex items-center gap-2">
                  <Badge variant={version.kind === "manual" ? "default" : "secondary"}>
                    {version.kind === "manual"
                      ? labels.historyKindManual
                      : labels.historyKindAutosave}
                  </Badge>
                  <time dateTime={version.createdAt} className="text-xs text-muted-foreground">
                    {when}
                  </time>
                </div>
                {version.label && <p className="font-medium">{version.label}</p>}
                <p className="text-xs text-muted-foreground">
                  {fillLabel(labels.historySize, { bytes: version.sizeBytes })}
                </p>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  aria-label={`${labels.restoreVersion}: ${version.label ?? when}`}
                  onClick={async () => setRestored(await history.restore(version.versionId))}
                >
                  {labels.restoreVersion}
                </Button>
              </li>
            );
          })}
        </ul>
      )}

      <Button
        type="button"
        variant="destructive"
        size="sm"
        disabled={history.locked && history.purgedCount !== null}
        onClick={() => void history.purgeForRedaction()}
      >
        {labels.purgeHistory}
      </Button>
    </aside>
  );
}
